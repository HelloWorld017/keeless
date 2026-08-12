use keeless_lesswire::{AuthenticatedSender, MessageFrame};
use keeless_schema::{KeyScope, Operation, OperationOutcome, OperationRequest, OperationResponse};

use crate::{CoreError, KeelessCore, MAX_REQUEST_ID_LENGTH, MAX_REQUEST_SIZE, Result, operations};

impl KeelessCore {
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
                .untrusted_server
                .take()
                .expect("untrusted server is present"),
            Target::Core => self.core_server.take().expect("core server was selected"),
        };
        let core_server_generation = self.core_server_generation;
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
            Target::Untrusted => self.untrusted_server = Some(server),
            Target::Core if self.core_server_generation == core_server_generation => {
                self.core_server = Some(server)
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
        self.transfer_owner = transfer_owner;
        self.authenticated_sender = sender;
        let outcome = match operations::execute(self, request.operation).await {
            Ok(success) => OperationOutcome::Success { success },
            Err(error) => OperationOutcome::Error {
                error: (&error).into(),
            },
        };
        self.transfer_owner = None;
        self.authenticated_sender = None;
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
    };
    let metadata = operation.metadata();
    metadata.senders.contains(&sender) && metadata.recipients.contains(&recipient)
}
