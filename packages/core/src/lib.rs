//! Host-independent Keeless plaintext operation state machine.

mod config;
mod credential;
mod database_state;
mod error;
mod extensions;
mod features;
mod host;
mod model;
mod network;
pub mod operations;
mod recent;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use config::{CONFIG_VERSION, PersistedConfig};
use credential::CredentialVault;
use database_state::{CONFIG_RECORD, EncryptedDatabaseStateStore};
use keeless_kdbx::CompositeKey;
#[cfg(test)]
use keeless_kdbx::SecureArray;
use keeless_lesswire::{Server, ServerHost, TransferId, TransferOwner, TransferRegistry};
use keeless_sync::{FileHandle, RemoteFile, StorageError, SyncReport};
use zeroize::Zeroizing;

pub use error::{CoreError, Result};
pub use host::{
    Clock, ConnectionApprovalKind, ConnectionApprovalProvider, ConnectionApprovalRequest,
    DatabasePersistence, HostFuture, KeelessHost, PasskeyConsentMode, PasskeyConsentProvider,
    PasskeyConsentRequest, PasswordInputMode, PasswordInputProvider, Storage, SystemClock,
    TaskSpawner, TransferProvider,
};
pub use keeless_schema;
pub use keeless_schema::{
    DatabaseStatus, KeelessConfig, KeelessConfigPatch, OperationError, StorageDescriptor,
    SyncStatus,
};
pub use keeless_sync::StorageProvider;
pub const MAX_REQUEST_SIZE: usize = 760 * 1024;
pub const MAX_REQUEST_ID_LENGTH: usize = 128;
const DATABASE_ID_HKDF_INFO: &[u8] = b"keeless database id v1";

/// Stable, non-secret identifier bound into database persistence authentication.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DatabaseId(Vec<u8>);

impl DatabaseId {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn from_storage(provider: &str, path: &str) -> Result<Self> {
        use hkdf::Hkdf;
        use sha2::Sha256;

        let mut input = Vec::with_capacity(provider.len() + path.len() + 16);
        input.extend_from_slice(&(provider.len() as u64).to_be_bytes());
        input.extend_from_slice(provider.as_bytes());
        input.extend_from_slice(&(path.len() as u64).to_be_bytes());
        input.extend_from_slice(path.as_bytes());
        let mut id = [0; 32];
        Hkdf::<Sha256>::new(None, &input)
            .expand(DATABASE_ID_HKDF_INFO, &mut id)
            .map_err(|_| CoreError::Crypto)?;
        Ok(Self(id.to_vec()))
    }

    pub(crate) fn from_recent_id(id: &str) -> Result<Self> {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

        URL_SAFE_NO_PAD
            .decode(id)
            .map(Self)
            .map_err(|_| CoreError::InvalidRecentDatabase)
    }

    pub(crate) fn recent_id(&self) -> String {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

        URL_SAFE_NO_PAD.encode(&self.0)
    }

    pub fn encoded(&self) -> String {
        self.recent_id()
    }
}

struct Selection {
    descriptor: Option<StorageDescriptor>,
    storage: Option<Arc<Storage>>,
    database_id: DatabaseId,
    exists: bool,
}

type BackgroundFetch = Arc<Mutex<Option<std::result::Result<RemoteFile, StorageError>>>>;

pub struct KeelessCore {
    untrusted_server: Option<Server>,
    core_server: Option<Server>,
    core_server_generation: u64,
    core_transfers: Option<TransferRegistry>,
    encrypted_state: Option<EncryptedDatabaseStateStore>,
    runtime_clients: Vec<String>,
    connection_approval: Arc<dyn ConnectionApprovalProvider>,
    password_input: Option<Arc<dyn PasswordInputProvider>>,
    passkey_consent: Option<Arc<dyn PasskeyConsentProvider>>,
    clock: Arc<dyn Clock>,
    storage_providers: HashMap<String, Arc<Storage>>,
    settings: KeelessConfig,
    selection: Option<Selection>,
    handle: Option<FileHandle>,
    credential: Option<CredentialVault>,
    extensions: extensions::Extensions,
    last_activity_ms: Option<u64>,
    persistence: Arc<dyn DatabasePersistence>,
    core_state: Arc<dyn keeless_lesswire::StateStore>,
    journal: Option<operations::mutations::MutationCoordinator>,
    sync_status: SyncStatus,
    sync_error: Option<OperationError>,
    pending_sync_key: Option<CompositeKey>,
    task_spawner: Option<Arc<dyn TaskSpawner>>,
    transfer_provider: Option<Arc<dyn TransferProvider>>,
    transfer_owner: Option<String>,
    authenticated_sender: Option<keeless_lesswire::AuthenticatedSender>,
    background_fetch: Option<BackgroundFetch>,
    background_started_ms: Option<u64>,
    dirty: bool,
}

impl std::fmt::Debug for KeelessCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeelessCore")
            .field("settings", &self.settings)
            .field("has_selection", &self.selection.is_some())
            .field("unlocked", &self.handle.is_some())
            .field("credential", &self.credential)
            .finish_non_exhaustive()
    }
}

impl KeelessCore {
    pub async fn new(host: KeelessHost) -> Result<Self> {
        let untrusted_server = Server::new(ServerHost {
            store: host.untrusted_state,
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
            encrypted_state: None,
            runtime_clients: Vec::new(),
            connection_approval: host.connection_approval,
            password_input: host.password_input,
            passkey_consent: host.passkey_consent,
            clock: host.clock,
            storage_providers: host.storage_providers,
            settings: KeelessConfig::default(),
            selection: None,
            handle: None,
            credential: None,
            extensions: extensions::Extensions::new()?,
            last_activity_ms: None,
            persistence: host.database_persistence,
            core_state: host.core_state,
            journal: None,
            sync_status: SyncStatus::Idle,
            sync_error: None,
            pending_sync_key: None,
            task_spawner: host.task_spawner,
            transfer_provider: host.transfer_provider,
            transfer_owner: None,
            authenticated_sender: None,
            background_fetch: None,
            background_started_ms: None,
            dirty: false,
        })
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

    pub(crate) async fn current_key(&self, password: Option<&[u8]>) -> Result<CompositeKey> {
        if self.handle.is_none() {
            return Err(CoreError::DatabaseLocked);
        }
        if let Some(password) = password {
            let key = CompositeKey::new().with_password(password)?;
            self.handle
                .as_ref()
                .ok_or(CoreError::DatabaseLocked)?
                .verify_credentials(&key)?;
            Ok(key)
        } else if let Some(credential) = &self.credential {
            credential.restore_key()
        } else {
            let password = self.request_password(PasswordInputMode::Save).await?;
            let key = CompositeKey::new().with_password(&password)?;
            self.handle
                .as_ref()
                .ok_or(CoreError::DatabaseLocked)?
                .verify_credentials(&key)?;
            Ok(key)
        }
    }

    pub async fn sync(&mut self, password: Option<&[u8]>) -> Result<SyncReport> {
        self.enforce_auto_lock();
        self.background_fetch = None;
        self.background_started_ms = None;
        self.pending_sync_key = None;
        let key = self.current_key(password).await?;
        self.sync_with_key(key, None).await
    }

    async fn sync_with_key(
        &mut self,
        key: CompositeKey,
        remote: Option<RemoteFile>,
    ) -> Result<SyncReport> {
        self.sync_status = SyncStatus::Syncing;
        self.sync_error = None;
        let handle = self.handle.as_mut().ok_or(CoreError::DatabaseLocked)?;
        let result = match remote {
            Some(remote) => handle.sync_from_remote(&key, remote).await,
            None => handle.sync(&key).await,
        };
        let report = match result {
            Ok(report) => report,
            Err(error) => {
                let error = CoreError::from(error);
                self.sync_status = SyncStatus::Error;
                self.sync_error = Some((&error).into());
                return Err(error);
            }
        };
        self.extensions.unlock(handle.database(), &key)?;
        let database = self
            .handle
            .as_ref()
            .ok_or(CoreError::DatabaseLocked)?
            .checkpoint_bytes()
            .to_vec();
        let cache = self
            .journal
            .as_ref()
            .ok_or(CoreError::DatabaseLocked)?
            .encode_cache(&database)?;
        if let Err(error) = self.persistence.write_cache(&cache).await {
            self.sync_status = SyncStatus::Error;
            self.sync_error = Some((&error).into());
            return Err(error);
        }
        if self
            .journal
            .as_ref()
            .is_some_and(|journal| journal.is_dirty())
        {
            if let Err(error) = self.persistence.clear_journal().await {
                self.sync_status = SyncStatus::Error;
                self.sync_error = Some((&error).into());
                return Err(error);
            }
            self.journal
                .as_mut()
                .expect("journal state checked")
                .mark_clean();
        }
        self.sync_status = SyncStatus::Idle;
        self.sync_error = None;
        self.dirty = false;
        self.last_activity_ms = Some(self.clock.monotonic_millis());
        Ok(report)
    }

    pub fn register_storage_provider(&mut self, name: impl Into<String>, storage: Arc<Storage>) {
        self.storage_providers.insert(name.into(), storage);
    }

    /// Adds a client approval for this host process without persisting it.
    ///
    /// The approval is applied to both endpoints and retained while the core
    /// endpoint is locked so it can be restored after the next unlock.
    pub fn add_runtime_client(&mut self, bundle: &str) -> Result<()> {
        let bundle = keeless_lesswire::PublicKeyBundle::parse(bundle)
            .ok_or_else(|| CoreError::Host("invalid runtime client bundle".into()))?;
        let bundle = bundle.as_str();
        self.untrusted_server
            .as_mut()
            .expect("untrusted server is restored after every frame")
            .add_runtime_approval(bundle)
            .map_err(|error| CoreError::Host(error.to_string()))?;
        if let Some(server) = self.core_server.as_mut() {
            server
                .add_runtime_approval(bundle)
                .map_err(|error| CoreError::Host(error.to_string()))?;
        }
        if !self.runtime_clients.iter().any(|client| client == bundle) {
            self.runtime_clients.push(bundle.into());
        }
        Ok(())
    }

    /// Revokes every process-local client approval from both endpoints.
    pub fn remove_runtime_clients(&mut self) {
        self.untrusted_server
            .as_mut()
            .expect("untrusted server is restored after every frame")
            .clear_runtime_approvals();
        if let Some(server) = self.core_server.as_mut() {
            server.clear_runtime_approvals();
        }
        self.runtime_clients.clear();
    }

    pub async fn tick(&mut self) {
        self.enforce_auto_lock();
        if self.handle.is_none() {
            self.pending_sync_key = None;
            self.background_fetch = None;
            self.background_started_ms = None;
            return;
        }

        if let Some(fetch) = &self.background_fetch {
            if self.background_started_ms.is_some_and(|started| {
                self.clock.monotonic_millis().saturating_sub(started) >= 30_000
            }) {
                self.background_fetch = None;
                self.background_started_ms = None;
                self.pending_sync_key = None;
                let error = CoreError::Host("background storage fetch timed out".into());
                self.sync_status = SyncStatus::Error;
                self.sync_error = Some((&error).into());
                return;
            }
            let completed = fetch.lock().ok().and_then(|mut result| result.take());
            if let Some(completed) = completed {
                self.background_fetch = None;
                self.background_started_ms = None;
                let Some(key) = self.pending_sync_key.take() else {
                    return;
                };
                match completed {
                    Ok(remote) => {
                        let _ = self.sync_with_key(key, Some(remote)).await;
                    }
                    Err(error) => {
                        let error = CoreError::from(error);
                        self.sync_status = SyncStatus::Error;
                        self.sync_error = Some((&error).into());
                    }
                }
            }
            return;
        }

        if let Some(key) = self.pending_sync_key.take() {
            let _ = self.sync_with_key(key, None).await;
            return;
        }

        if self.handle.as_ref().is_some_and(|handle| handle.is_dirty())
            && self.pending_sync_key.is_none()
            && let Some(credential) = &self.credential
            && let Ok(key) = credential.restore_key()
        {
            self.start_background_sync(key);
        }
    }

    fn start_background_sync(&mut self, key: CompositeKey) {
        let Some(spawner) = &self.task_spawner else {
            self.pending_sync_key = Some(key);
            return;
        };
        let Some(selection) = &self.selection else {
            return;
        };
        let Some(storage) = selection.storage.as_ref().cloned() else {
            return;
        };
        let Some(path) = selection
            .descriptor
            .as_ref()
            .map(|descriptor| descriptor.path.clone())
        else {
            return;
        };
        let result = Arc::new(Mutex::new(None));
        let task_result = Arc::clone(&result);
        spawner.spawn(Box::pin(async move {
            let fetched = storage.provider().read(&path, None).await;
            if let Ok(mut result) = task_result.lock() {
                *result = Some(fetched);
            }
        }));
        self.pending_sync_key = Some(key);
        self.background_fetch = Some(result);
        self.background_started_ms = Some(self.clock.monotonic_millis());
        self.sync_status = SyncStatus::Syncing;
        self.sync_error = None;
    }

    fn enforce_auto_lock(&mut self) {
        let Some(timeout) = self.settings.auto_lock_timeout_ms else {
            return;
        };
        let Some(last_activity) = self.last_activity_ms else {
            return;
        };
        if self.clock.monotonic_millis().saturating_sub(last_activity) >= timeout {
            operations::lock::run(self);
        }
    }

    fn touch_activity(&mut self) {
        if self.handle.is_some() {
            self.last_activity_ms = Some(self.clock.monotonic_millis());
        }
    }

    async fn request_password(&self, mode: PasswordInputMode) -> Result<Zeroizing<Vec<u8>>> {
        let provider = self
            .password_input
            .as_ref()
            .ok_or(CoreError::PasswordRequired)?;
        provider
            .request_password(mode)
            .await?
            .ok_or(CoreError::PasswordRequired)
    }

    pub(crate) async fn request_passkey_consent(
        &self,
        request: PasskeyConsentRequest,
    ) -> Result<usize> {
        let provider = self
            .passkey_consent
            .as_ref()
            .ok_or(CoreError::PasskeyConsentRequired)?;
        provider
            .request_passkey_consent(request)
            .await?
            .ok_or(CoreError::PasskeyConsentDenied)
    }

    async fn persist(&self) -> Result<()> {
        let state = self
            .encrypted_state
            .as_ref()
            .ok_or(CoreError::DatabaseLocked)?;
        let persisted = PersistedConfig {
            version: CONFIG_VERSION,
            settings: self.settings.clone(),
            storage: self.selection.as_ref().and_then(|selection| {
                selection
                    .storage
                    .as_ref()
                    .filter(|storage| storage.is_persistent())
                    .and_then(|_| selection.descriptor.clone())
            }),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&persisted)?);
        state.save_record(CONFIG_RECORD, &bytes).await
    }

    pub(crate) async fn activate_database_state(
        &mut self,
        raw_key: &keeless_kdbx::SecureArray<32>,
    ) -> Result<()> {
        let persistence = self.persistence.clone();
        let database_id = self
            .selection
            .as_ref()
            .ok_or(CoreError::NoDatabaseSelected)?
            .database_id
            .clone();
        let state = EncryptedDatabaseStateStore::new(raw_key, persistence, database_id)?;
        let persisted = match state.load_record(CONFIG_RECORD).await? {
            Some(bytes) => {
                let persisted: PersistedConfig = serde_json::from_slice(&bytes)
                    .map_err(|error| CoreError::InvalidConfig(error.to_string()))?;
                persisted.validate()?;
                persisted
            }
            None => {
                let persisted = PersistedConfig {
                    version: CONFIG_VERSION,
                    settings: KeelessConfig::default(),
                    storage: None,
                };
                state
                    .save_record(
                        CONFIG_RECORD,
                        &Zeroizing::new(serde_json::to_vec(&persisted)?),
                    )
                    .await?;
                persisted
            }
        };
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
        self.settings = persisted.settings;
        self.core_transfers = Some(server.transfers());
        self.encrypted_state = Some(state);
        self.core_server_generation = self.core_server_generation.wrapping_add(1);
        self.core_server = Some(server);
        Ok(())
    }

    pub(crate) async fn restore_recent_selection(
        &mut self,
        raw_key: &keeless_kdbx::SecureArray<32>,
    ) -> Result<()> {
        let database_id = self
            .selection
            .as_ref()
            .ok_or(CoreError::NoDatabaseSelected)?
            .database_id
            .clone();
        if self
            .selection
            .as_ref()
            .is_some_and(|selection| selection.descriptor.is_some())
        {
            return Ok(());
        }
        let state = EncryptedDatabaseStateStore::new(
            raw_key,
            self.persistence.clone(),
            database_id.clone(),
        )?;
        let bytes = state
            .load_record(CONFIG_RECORD)
            .await?
            .ok_or(CoreError::RecentDatabaseUnavailable)?;
        let persisted: PersistedConfig = serde_json::from_slice(&bytes)
            .map_err(|error| CoreError::InvalidConfig(error.to_string()))?;
        persisted.validate()?;
        let descriptor = persisted
            .storage
            .ok_or(CoreError::RecentDatabaseUnavailable)?;
        let storage = self
            .storage_providers
            .get(&descriptor.provider)
            .cloned()
            .ok_or_else(|| CoreError::UnknownStorageProvider(descriptor.provider.clone()))?;
        let normalized_path = storage.get_normalized_path(&descriptor.path)?;
        let resolved_id = DatabaseId::from_storage(&descriptor.provider, &normalized_path)?;
        if resolved_id != database_id {
            return Err(CoreError::RecentDatabaseMismatch);
        }
        let selection = self.selection.as_mut().expect("selection was checked");
        selection.descriptor = Some(descriptor);
        selection.storage = Some(storage);
        Ok(())
    }

    pub fn untrusted_public_key_bundle(&self) -> String {
        self.untrusted_server
            .as_ref()
            .expect("untrusted server is restored after every frame")
            .public_key_bundle()
    }

    pub fn core_public_key_bundle(&self) -> Option<String> {
        self.core_server.as_ref().map(Server::public_key_bundle)
    }
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

fn random_array<const N: usize>() -> Result<[u8; N]> {
    let mut value = [0; N];
    getrandom::getrandom(&mut value).map_err(|_| CoreError::Crypto)?;
    Ok(value)
}

#[cfg(test)]
mod tests;
