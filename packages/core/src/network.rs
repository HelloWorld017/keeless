use keeless_schema::{MessageFrame, OperationOutcome, OperationRequest, OperationResponse};
use zeroize::Zeroizing;

use crate::{
    KeelessCore, Result, operations,
    protocol::{
        MAX_FRAME_SIZE, MAX_REQUEST_ID_LENGTH, MAX_REQUEST_SIZE, decrypt_frame, encrypt_frame,
        handshake_frame,
    },
};

impl KeelessCore {
    pub async fn process_frame(&mut self, frame: &MessageFrame) -> Result<Option<MessageFrame>> {
        let frame_size = frame
            .nonce
            .len()
            .saturating_add(frame.ephemeral_public_key.as_ref().map_or(0, String::len))
            .saturating_add(frame.public_key.len())
            .saturating_add(frame.payload.as_ref().map_or(0, String::len))
            .saturating_add(frame.signature.len())
            .saturating_add(128);
        if frame_size > MAX_FRAME_SIZE {
            return Ok(None);
        }

        let Some(sender) = self.authenticate_frame(frame).await? else {
            return Ok(None);
        };
        if frame.payload.is_none() {
            let signing = self.identity.signing_key()?;
            return handshake_frame(self.clock.now_millis(), self.identity.bundle()?, &signing)
                .map(Some);
        }

        let Some(plaintext) = decrypt_frame(frame, &self.identity.x25519()?) else {
            return Ok(None);
        };
        let plaintext = Zeroizing::new(plaintext);
        if plaintext.len() > MAX_REQUEST_SIZE {
            return Ok(None);
        }
        let Ok(request) = serde_json::from_slice::<OperationRequest>(&plaintext) else {
            return Ok(None);
        };
        if request.request_id.is_empty() || request.request_id.len() > MAX_REQUEST_ID_LENGTH {
            return Ok(None);
        }

        let response = self.dispatch(request).await;
        let response = serde_json::to_vec(&response)?;
        let signing = self.identity.signing_key()?;
        encrypt_frame(
            self.clock.now_millis(),
            &response,
            self.identity.bundle()?,
            &signing,
            &sender.encryption,
        )
        .map(Some)
    }

    pub async fn process_frame_json(&mut self, bytes: &[u8]) -> Result<Option<Vec<u8>>> {
        if bytes.len() > MAX_FRAME_SIZE {
            return Ok(None);
        }
        let Ok(frame) = serde_json::from_slice::<MessageFrame>(bytes) else {
            return Ok(None);
        };
        self.process_frame(&frame)
            .await?
            .map(|response| serde_json::to_vec(&response).map_err(Into::into))
            .transpose()
    }

    async fn dispatch(&mut self, request: OperationRequest) -> OperationResponse {
        self.enforce_auto_lock();
        let request_id = request.request_id;
        let outcome = match operations::execute(self, request.operation).await {
            Ok(success) => {
                if self.handle.is_some() {
                    self.last_activity_ms = Some(self.clock.monotonic_millis());
                }
                OperationOutcome::Success { success }
            }
            Err(error) => OperationOutcome::Error {
                error: (&error).into(),
            },
        };
        OperationResponse {
            request_id,
            outcome,
        }
    }
}
