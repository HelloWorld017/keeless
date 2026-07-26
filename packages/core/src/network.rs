use keeless_schema::{OperationOutcome, OperationRequest, OperationResponse};

use crate::{KeelessCore, MAX_REQUEST_ID_LENGTH, MAX_REQUEST_SIZE, Result, operations};

impl KeelessCore {
    pub async fn handle_payload(&mut self, plaintext: &[u8]) -> Result<Option<Vec<u8>>> {
        if plaintext.len() > MAX_REQUEST_SIZE {
            return Ok(None);
        }
        let Ok(request) = serde_json::from_slice::<OperationRequest>(plaintext) else {
            return Ok(None);
        };
        if request.request_id.is_empty() || request.request_id.len() > MAX_REQUEST_ID_LENGTH {
            return Ok(None);
        }

        let response = self.dispatch(request).await;
        serde_json::to_vec(&response).map(Some).map_err(Into::into)
    }

    async fn dispatch(&mut self, request: OperationRequest) -> OperationResponse {
        self.enforce_auto_lock();
        let request_id = request.request_id;
        let outcome = match operations::execute(self, request.operation).await {
            Ok(success) => OperationOutcome::Success { success },
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
