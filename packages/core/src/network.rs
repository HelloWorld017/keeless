use std::sync::Arc;

use keeless_lesswire::{
    AuthenticatedSender, MessageFrame, Server, ServerHost, TransferId, TransferOwner,
    TransferRegistry,
};
use keeless_schema::{KeyScope, Operation, OperationOutcome, OperationRequest, OperationResponse};
use zeroize::Zeroizing;

use crate::{
    Clock, ConnectionApprovalKind, ConnectionApprovalProvider, ConnectionApprovalRequest,
    CoreError, KeelessCore, KeelessHost, MAX_REQUEST_ID_LENGTH, MAX_REQUEST_SIZE, Result,
    TransferProvider, database_state::EncryptedDatabaseStateStore,
};

pub(crate) struct Network {
    untrusted_server: Option<Server>,
    core_server: Option<Server>,
    core_server_generation: u64,
    core_transfers: Option<TransferRegistry>,
    runtime_clients: Vec<String>,
    connection_approval: Arc<dyn ConnectionApprovalProvider>,
    clock: Arc<dyn Clock>,
    transfer_provider: Option<Arc<dyn TransferProvider>>,
    transfer_owner: Option<String>,
    authenticated_sender: Option<AuthenticatedSender>,
}

impl Network {
    pub(crate) async fn new(host: &KeelessHost) -> Result<Self> {
        let untrusted_server = Server::new(ServerHost {
            store: Arc::clone(&host.untrusted_state),
            approval_provider: Arc::new(HostApprovalAdapter(host.connection_approval.clone())),
            clock: Arc::new(WireClockAdapter(host.clock.clone())),
            scope: keeless_lesswire::KeyScope::CoreUntrusted,
            allow_transfers: false,
            runtime_approved_clients: Vec::new(),
        })
        .await
        .map_err(|error| CoreError::Host(error.to_string()))?;

        Ok(Self {
            untrusted_server: Some(untrusted_server),
            core_server: None,
            core_server_generation: 0,
            core_transfers: None,
            runtime_clients: Vec::new(),
            connection_approval: host.connection_approval.clone(),
            clock: host.clock.clone(),
            transfer_provider: host.transfer_provider.clone(),
            transfer_owner: None,
            authenticated_sender: None,
        })
    }

    pub(crate) async fn activate_core_server(
        &mut self,
        state: &EncryptedDatabaseStateStore,
    ) -> Result<()> {
        let server = Server::new(ServerHost {
            store: Arc::new(state.clone()),
            approval_provider: Arc::new(HostApprovalAdapter(self.connection_approval.clone())),
            clock: Arc::new(WireClockAdapter(self.clock.clone())),
            scope: keeless_lesswire::KeyScope::Core,
            allow_transfers: true,
            runtime_approved_clients: self.runtime_clients.clone(),
        })
        .await
        .map_err(|error| CoreError::Host(error.to_string()))?;
        self.core_transfers = Some(server.transfers());
        self.core_server_generation = self.core_server_generation.wrapping_add(1);
        self.core_server = Some(server);
        Ok(())
    }

    pub(crate) fn drop_core_server(&mut self) {
        self.core_server_generation = self.core_server_generation.wrapping_add(1);
        self.core_server = None;
        self.core_transfers = None;
    }

    pub(crate) fn clear_transfers(&self) {
        if self.transfer_provider.is_none()
            && let Some(transfers) = &self.core_transfers
        {
            transfers.clear();
        }
        if let Some(provider) = &self.transfer_provider {
            provider.clear();
        }
    }

    pub(crate) fn publish_download_transfer(&self, bytes: Zeroizing<Vec<u8>>) -> Result<String> {
        if self.transfer_provider.is_none()
            && let Some(transfers) = &self.core_transfers
        {
            let owner = self
                .transfer_owner
                .as_deref()
                .ok_or_else(|| CoreError::Host("binary transfer owner is unavailable".into()))?;
            return transfers
                .publish_download(TransferOwner::new(owner), bytes)
                .map(|id| id.encode())
                .map_err(|error| CoreError::Host(error.to_string()));
        }
        let provider = self
            .transfer_provider
            .as_ref()
            .ok_or_else(|| CoreError::Host("binary transfers are unavailable".into()))?;
        let owner = self
            .transfer_owner
            .as_deref()
            .ok_or_else(|| CoreError::Host("binary transfer owner is unavailable".into()))?;
        provider.publish_download(owner, bytes)
    }

    pub(crate) fn consume_upload_transfer(&self, transfer_id: &str) -> Result<Zeroizing<Vec<u8>>> {
        if self.transfer_provider.is_none()
            && let Some(transfers) = &self.core_transfers
        {
            let owner = self
                .transfer_owner
                .as_deref()
                .ok_or_else(|| CoreError::Host("binary transfer owner is unavailable".into()))?;
            let id = TransferId::parse(transfer_id)
                .ok_or_else(|| CoreError::Host("invalid binary transfer ID".into()))?;
            return transfers
                .consume_upload(&TransferOwner::new(owner), &id)
                .map_err(|error| CoreError::Host(error.to_string()));
        }
        let provider = self
            .transfer_provider
            .as_ref()
            .ok_or_else(|| CoreError::Host("binary transfers are unavailable".into()))?;
        let owner = self
            .transfer_owner
            .as_deref()
            .ok_or_else(|| CoreError::Host("binary transfer owner is unavailable".into()))?;
        provider.consume_upload(owner, transfer_id)
    }

    pub(crate) fn authenticated_sender(&self) -> Option<AuthenticatedSender> {
        self.authenticated_sender.clone()
    }

    #[cfg(test)]
    pub(crate) fn set_authenticated_sender(&mut self, sender: Option<AuthenticatedSender>) {
        self.authenticated_sender = sender;
    }

    #[cfg(test)]
    pub(crate) fn set_transfer_provider(&mut self, provider: Option<Arc<dyn TransferProvider>>) {
        self.transfer_provider = provider;
    }

    #[cfg(test)]
    pub(crate) fn set_transfer_owner(&mut self, owner: Option<String>) {
        self.transfer_owner = owner;
    }

    pub(crate) async fn upgrade_sender(&mut self, sender: &AuthenticatedSender) -> Result<String> {
        let server = self.core_server.as_mut().ok_or(CoreError::DatabaseLocked)?;
        let recipient = server.public_key_bundle();
        match sender.approval {
            keeless_lesswire::SenderApproval::Runtime => {
                server.add_runtime_approval(&sender.public_key_bundle)
            }
            keeless_lesswire::SenderApproval::Persisted => {
                server.approve_upgrade(&sender.public_key_bundle).await
            }
        }
        .map_err(|error| CoreError::Host(error.to_string()))?;
        Ok(recipient)
    }
}

impl KeelessCore {
    pub(crate) fn publish_download_transfer(&self, bytes: Zeroizing<Vec<u8>>) -> Result<String> {
        self.network.publish_download_transfer(bytes)
    }

    pub(crate) fn consume_upload_transfer(&self, transfer_id: &str) -> Result<Zeroizing<Vec<u8>>> {
        self.network.consume_upload_transfer(transfer_id)
    }

    pub(crate) fn clear_transfers(&self) {
        self.network.clear_transfers();
    }

    pub(crate) fn drop_core_server(&mut self) {
        self.network.drop_core_server();
    }

    /// Adds a client approval for this host process without persisting it.
    ///
    /// The approval is applied to both endpoints and retained while the core
    /// endpoint is locked so it can be restored after the next unlock.
    pub fn add_runtime_client(&mut self, bundle: &str) -> Result<()> {
        let bundle = keeless_lesswire::PublicKeyBundle::parse(bundle)
            .ok_or_else(|| CoreError::Host("invalid runtime client bundle".into()))?;
        let bundle = bundle.as_str();
        self.network
            .untrusted_server
            .as_mut()
            .expect("untrusted server is restored after every frame")
            .add_runtime_approval(bundle)
            .map_err(|error| CoreError::Host(error.to_string()))?;
        if let Some(server) = self.network.core_server.as_mut() {
            server
                .add_runtime_approval(bundle)
                .map_err(|error| CoreError::Host(error.to_string()))?;
        }
        if !self
            .network
            .runtime_clients
            .iter()
            .any(|client| client == bundle)
        {
            self.network.runtime_clients.push(bundle.into());
        }
        Ok(())
    }

    /// Revokes every process-local client approval from both endpoints.
    pub fn remove_runtime_clients(&mut self) {
        self.network
            .untrusted_server
            .as_mut()
            .expect("untrusted server is restored after every frame")
            .clear_runtime_approvals();
        if let Some(server) = self.network.core_server.as_mut() {
            server.clear_runtime_approvals();
        }
        self.network.runtime_clients.clear();
    }

    pub fn untrusted_public_key_bundle(&self) -> String {
        self.network
            .untrusted_server
            .as_ref()
            .expect("untrusted server is restored after every frame")
            .public_key_bundle()
    }

    pub fn core_public_key_bundle(&self) -> Option<String> {
        self.network
            .core_server
            .as_ref()
            .map(Server::public_key_bundle)
    }

    pub(crate) fn authenticated_sender(&self) -> Option<AuthenticatedSender> {
        self.network.authenticated_sender()
    }

    #[cfg(test)]
    pub(crate) fn set_authenticated_sender(&mut self, sender: Option<AuthenticatedSender>) {
        self.network.set_authenticated_sender(sender);
    }

    #[cfg(test)]
    pub(crate) fn set_transfer_provider(&mut self, provider: Option<Arc<dyn TransferProvider>>) {
        self.network.set_transfer_provider(provider);
    }

    #[cfg(test)]
    pub(crate) fn set_transfer_owner(&mut self, owner: Option<String>) {
        self.network.set_transfer_owner(owner);
    }

    pub(crate) async fn upgrade_sender(&mut self, sender: &AuthenticatedSender) -> Result<String> {
        self.network.upgrade_sender(sender).await
    }

    pub async fn handle_frame(&mut self, bytes: &[u8]) -> Result<Option<Vec<u8>>> {
        if bytes.len() > keeless_lesswire::MAX_FRAME_SIZE {
            return Ok(None);
        }
        let Ok(frame) = serde_json::from_slice::<MessageFrame>(bytes) else {
            return Ok(None);
        };
        enum Target {
            Untrusted,
            Core,
        }
        let target = if frame.recipient == self.untrusted_public_key_bundle() {
            Target::Untrusted
        } else if self
            .network
            .core_server
            .as_ref()
            .is_some_and(|server| frame.recipient == server.public_key_bundle())
        {
            Target::Core
        } else {
            return Ok(None);
        };
        let recipient_scope = match target {
            Target::Untrusted => KeyScope::CoreUntrusted,
            Target::Core => KeyScope::Core,
        };
        let mut server = match target {
            Target::Untrusted => self
                .network
                .untrusted_server
                .take()
                .expect("untrusted server is present"),
            Target::Core => self
                .network
                .core_server
                .take()
                .expect("core server was selected"),
        };
        let core_server_generation = self.network.core_server_generation;
        let response = server
            .handle_frame(&frame, |sender, plaintext| {
                let core = &mut *self;
                async move {
                    core.handle_payload_with_context(
                        Some(sender.public_key_bundle.clone()),
                        Some(sender),
                        Some(recipient_scope),
                        &plaintext,
                    )
                    .await
                }
            })
            .await;
        match target {
            Target::Untrusted => self.network.untrusted_server = Some(server),
            Target::Core if self.network.core_server_generation == core_server_generation => {
                self.network.core_server = Some(server)
            }
            Target::Core => {}
        }
        let response = response.map_err(|error| CoreError::Host(error.to_string()))?;
        response
            .map(|frame| serde_json::to_vec(&frame).map_err(Into::into))
            .transpose()
    }

    async fn handle_payload_with_context(
        &mut self,
        transfer_owner: Option<String>,
        sender: Option<AuthenticatedSender>,
        recipient_scope: Option<KeyScope>,
        plaintext: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        if plaintext.len() > MAX_REQUEST_SIZE {
            return Ok(None);
        }
        let Ok(request) = serde_json::from_slice::<OperationRequest>(plaintext) else {
            return Ok(None);
        };
        if request.request_id.is_empty() || request.request_id.len() > MAX_REQUEST_ID_LENGTH {
            return Ok(None);
        }
        if let (Some(sender), Some(recipient)) = (&sender, recipient_scope)
            && !operation_allowed(&request.operation, sender.scope, recipient)
        {
            return Ok(None);
        }

        self.enforce_auto_lock();
        let request_id = request.request_id;
        self.network.transfer_owner = transfer_owner;
        self.network.authenticated_sender = sender;
        let outcome = match crate::operations::execute(self, request.operation).await {
            Ok(success) => OperationOutcome::Success { success },
            Err(error) => OperationOutcome::Error {
                error: (&error).into(),
            },
        };
        self.network.transfer_owner = None;
        self.network.authenticated_sender = None;
        let response = OperationResponse {
            request_id,
            outcome,
        };
        let bytes = serde_json::to_vec(&response)?;
        if bytes.len() <= MAX_REQUEST_SIZE {
            return Ok(Some(bytes));
        }
        let response = OperationResponse {
            request_id: response.request_id,
            outcome: OperationOutcome::Error {
                error: (&CoreError::Host("operation response exceeds the payload limit".into()))
                    .into(),
            },
        };
        serde_json::to_vec(&response).map(Some).map_err(Into::into)
    }
}

fn operation_allowed(
    operation: &Operation,
    sender: keeless_lesswire::KeyScope,
    recipient: KeyScope,
) -> bool {
    let sender = match sender {
        keeless_lesswire::KeyScope::CoreUntrusted => KeyScope::CoreUntrusted,
        keeless_lesswire::KeyScope::Core => KeyScope::Core,
        keeless_lesswire::KeyScope::App => KeyScope::App,
        keeless_lesswire::KeyScope::Passkey => KeyScope::Passkey,
        keeless_lesswire::KeyScope::NativeUi => KeyScope::NativeUi,
    };
    let metadata = operation.metadata();
    metadata.senders.contains(&sender) && metadata.recipients.contains(&recipient)
}

struct WireClockAdapter(Arc<dyn Clock>);

impl keeless_lesswire::Clock for WireClockAdapter {
    fn now_millis(&self) -> i64 {
        self.0.now_millis()
    }

    fn monotonic_millis(&self) -> u64 {
        self.0.monotonic_millis()
    }
}

struct HostApprovalAdapter(Arc<dyn ConnectionApprovalProvider>);

impl keeless_lesswire::ApprovalProvider for HostApprovalAdapter {
    fn approve(
        &self,
        request: keeless_lesswire::ApprovalRequest,
    ) -> keeless_lesswire::WireFuture<'_, keeless_lesswire::Result<bool>> {
        Box::pin(async move {
            self.0
                .approve_connection(ConnectionApprovalRequest {
                    sender: request.sender,
                    sender_scope: request.sender_scope,
                    recipient: request.recipient,
                    recipient_scope: request.recipient_scope,
                    kind: match request.kind {
                        keeless_lesswire::ApprovalKind::Initial => ConnectionApprovalKind::Initial,
                        keeless_lesswire::ApprovalKind::Upgrade => ConnectionApprovalKind::Upgrade,
                    },
                })
                .await
                .map_err(|error| keeless_lesswire::Error::Host(error.to_string()))
        })
    }
}
