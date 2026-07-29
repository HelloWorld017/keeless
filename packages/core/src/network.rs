use keeless_schema::{OperationOutcome, OperationRequest, OperationResponse};

use crate::{CoreError, KeelessCore, MAX_REQUEST_ID_LENGTH, MAX_REQUEST_SIZE, Result, operations};

impl KeelessCore {
    pub async fn handle_payload(&mut self, plaintext: &[u8]) -> Result<Option<Vec<u8>>> {
        self.handle_payload_from(None, plaintext).await
    }

    pub async fn handle_payload_from(
        &mut self,
        transfer_owner: Option<String>,
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

        let response = self.dispatch(request, transfer_owner).await;
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

    async fn dispatch(
        &mut self,
        request: OperationRequest,
        transfer_owner: Option<String>,
    ) -> OperationResponse {
        self.enforce_auto_lock();
        let request_id = request.request_id;
        self.transfer_owner = transfer_owner;
        let outcome = match operations::execute(self, request.operation).await {
            Ok(success) => OperationOutcome::Success { success },
            Err(error) => OperationOutcome::Error {
                error: (&error).into(),
            },
        };
        self.transfer_owner = None;
        OperationResponse {
            request_id,
            outcome,
        }
    }
}
