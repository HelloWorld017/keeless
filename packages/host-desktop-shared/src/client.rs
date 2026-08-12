//! Operation-level client for the running Keeless desktop host.

use std::sync::Arc;

use keeless_lesswire::{Client as WireClient, MessageFrame, SystemClock};
use keeless_schema::{
    Operation, OperationOutcome, OperationRequest, OperationResponse, OperationSuccess,
};

use crate::ipc::{self, IpcError, Request, Response};
use crate::launcher::LauncherError;
use crate::state::{ClientState, StateError};

/// Stable ID for the host's bootstrap endpoint.
pub const UNTRUSTED_ENDPOINT_ID: &str = "untrusted";
/// Stable ID for the host's upgraded core endpoint.
pub const CORE_ENDPOINT_ID: &str = "core";

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error(transparent)]
    Ipc(#[from] IpcError),
    #[error(transparent)]
    Wire(#[from] keeless_lesswire::Error),
    #[error(transparent)]
    State(#[from] StateError),
    #[error(transparent)]
    Launcher(#[from] LauncherError),
    #[error("host response could not be parsed: {0}")]
    Malformed(#[from] serde_json::Error),
    /// The host dropped the frame, which means it no longer trusts this client.
    #[error("host rejected the request; pair with the app again")]
    Rejected,
    #[error("host answered a different request")]
    MismatchedResponse,
    #[error("the server identity for this endpoint changed; reset pairing to continue")]
    ServerIdentityChanged,
    /// The host ran the operation and refused it. `code` matches the core's
    /// operation error codes, so callers can map it onto their own protocol.
    #[error("host reported {code}: {message}")]
    Operation { code: String, message: String },
}

/// Sends Keeless operations to the desktop host over authenticated, encrypted frames.
///
/// Each request opens its own connection, matching the host's one-request-per-
/// connection listener, and the frame is built immediately before it is written so
/// a slow connect cannot push it outside the protocol's freshness window.
pub struct CoreClient {
    wire: WireClient,
    session: String,
    next_request: u64,
}

impl CoreClient {
    /// Connect to a stable endpoint and optional advertised recipient.
    ///
    /// Pairing sends a handshake the host answers only after the user approves this
    /// client, and the host key learned that way is pinned in `state`.
    pub async fn connect(
        state: &mut ClientState,
        endpoint_id: &str,
        recipient: Option<String>,
    ) -> Result<Self, ClientError> {
        let recipient = match recipient {
            Some(recipient) => recipient,
            None => ipc::Client::bootstrap().await?,
        };
        if let Some(pinned) = state.trusted_server(endpoint_id) {
            if pinned != recipient {
                return Err(ClientError::ServerIdentityChanged);
            }
        } else {
            let mut wire = WireClient::new(state.identity()?, &recipient, Arc::new(SystemClock))?;
            let mut connection = ipc::Client::connect().await?;
            let frame = serde_json::to_vec(&wire.handshake_frame()?)?;
            connection
                .send_request(&Request::HandleFrame(frame))
                .await?;
            let response = expect_frame(connection.receive_response().await?)?;
            wire.accept_handshake(&parse_frame(&response)?)?;
            state
                .set_trusted_server(endpoint_id.into(), recipient.clone())
                .await?;
        }
        let wire = WireClient::new(state.identity()?, &recipient, Arc::new(SystemClock))?;

        let mut session = [0_u8; 8];
        getrandom::getrandom(&mut session)
            .map_err(|error| ClientError::State(StateError::Io(error.into())))?;
        Ok(Self {
            wire,
            session: hex::encode(session),
            next_request: 0,
        })
    }

    /// Check that the host is reachable without sending an operation.
    pub async fn ping() -> Result<(), ClientError> {
        match ipc::Client::request(Request::Ping).await? {
            Response::Pong => Ok(()),
            _ => Err(IpcError::Protocol("unexpected ping response".into()).into()),
        }
    }

    pub async fn request(&mut self, operation: Operation) -> Result<OperationSuccess, ClientError> {
        if self.wire.public_key_bundle().ends_with(".passkey")
            && !matches!(
                operation,
                Operation::GetPasskeys(_)
                    | Operation::RegisterPasskey(_)
                    | Operation::AssertPasskey(_)
            )
        {
            return Err(ClientError::Rejected);
        }
        self.next_request += 1;
        let request_id = format!("{}-{}", self.session, self.next_request);
        let payload = serde_json::to_vec(&OperationRequest {
            request_id: request_id.clone(),
            operation,
        })?;

        let mut connection = ipc::Client::connect().await?;
        let frame = serde_json::to_vec(&self.wire.encrypt(&payload)?)?;
        connection
            .send_request(&Request::HandleFrame(frame))
            .await?;
        let response = expect_frame(connection.receive_response().await?)?;

        let plaintext = self
            .wire
            .decrypt(&parse_frame(&response)?)?
            .ok_or(ClientError::Rejected)?;
        let response: OperationResponse = serde_json::from_slice(&plaintext)?;
        if response.request_id != request_id {
            return Err(ClientError::MismatchedResponse);
        }
        match response.outcome {
            OperationOutcome::Success { success } => Ok(success),
            OperationOutcome::Error { error } => Err(ClientError::Operation {
                code: error.code,
                message: error.message,
            }),
        }
    }
}

fn expect_frame(response: Response) -> Result<Vec<u8>, ClientError> {
    match response {
        Response::Frame(Some(frame)) => Ok(frame),
        Response::Frame(None) => Err(ClientError::Rejected),
        _ => Err(IpcError::Protocol("unexpected frame response".into()).into()),
    }
}

fn parse_frame(bytes: &[u8]) -> Result<MessageFrame, ClientError> {
    Ok(serde_json::from_slice(bytes)?)
}
