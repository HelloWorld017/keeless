//! Turning CTAP2 commands into Keeless operations.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use keeless_host_desktop_shared::ClientError;
use keeless_passkey_ctap::error::{CtapError, CtapStatus};
use keeless_passkey_ctap::request::{Command, GetAssertionRequest, MakeCredentialRequest};
use keeless_passkey_ctap::response::{self, Assertion, AuthenticatorInfo};
use keeless_passkey_ctap::{KEELESS_AAGUID, parse_command};
use keeless_schema::{AssertPasskeyArgs, Operation, OperationSuccess, RegisterPasskeyArgs};

use crate::ctaphid::KeepaliveStatus;
use crate::session::Session;

/// Progress a running command reports, so the transport can keep the host informed.
///
/// Shared rather than returned because the transport must keep sending keepalive
/// packets while the command is still running.
#[derive(Clone, Default)]
pub struct Progress(Arc<AtomicU8>);

impl Progress {
    pub fn status(&self) -> KeepaliveStatus {
        if self.0.load(Ordering::Relaxed) == 1 {
            KeepaliveStatus::UserPresenceNeeded
        } else {
            KeepaliveStatus::Processing
        }
    }

    fn waiting_for_user(&self, waiting: bool) {
        self.0.store(u8::from(waiting), Ordering::Relaxed);
    }
}

/// Serves CTAP2 commands from the passkeys held by the desktop app.
pub struct Authenticator {
    session: Session,
    /// Transports to advertise in `authenticatorGetInfo`.
    transports: &'static [&'static str],
}

impl Authenticator {
    pub fn new(session: Session) -> Self {
        Self {
            session,
            transports: &["usb"],
        }
    }

    /// Run one CTAP2 command, always answering with an encoded payload.
    pub async fn handle(&mut self, payload: &[u8], progress: &Progress) -> Vec<u8> {
        progress.waiting_for_user(false);
        let result = match parse_command(payload) {
            Ok(Command::MakeCredential(request)) => self.make_credential(*request, progress).await,
            Ok(Command::GetAssertion(request)) => self.get_assertion(request, progress).await,
            Ok(Command::GetInfo) => self.get_info(),
            Ok(Command::Selection) => self.selection(progress).await,
            // Every assertion this authenticator returns is already the one the
            // user picked, so there is never a next one.
            Ok(Command::GetNextAssertion) => Err(CtapStatus::NotAllowed.into()),
            // Credentials live in the user's database, not on the authenticator.
            Ok(Command::Reset) => Err(CtapStatus::OperationDenied.into()),
            Err(error) => Err(error),
        };
        progress.waiting_for_user(false);
        match result {
            Ok(encoded) => encoded,
            Err(error) => response::status(error.status),
        }
    }

    fn get_info(&self) -> Result<Vec<u8>, CtapError> {
        response::get_info(&AuthenticatorInfo {
            aaguid: &KEELESS_AAGUID,
            // A virtual HID device is indistinguishable from a removable key,
            // and claiming otherwise would misinform the platform's UI.
            platform_device: false,
            transports: self.transports,
        })
    }

    async fn selection(&mut self, progress: &Progress) -> Result<Vec<u8>, CtapError> {
        let _ = progress;
        Ok(response::status(CtapStatus::Success))
    }

    async fn make_credential(
        &mut self,
        request: MakeCredentialRequest,
        progress: &Progress,
    ) -> Result<Vec<u8>, CtapError> {
        let result = self
            .request_interactive(
                progress,
                Operation::RegisterPasskey(RegisterPasskeyArgs {
                    rp_id: request.rp_id,
                    rp_name: request.rp_name,
                    user_name: request.user_name,
                    user_handle: encode(&request.user_id),
                    client_data_hash: encode(&request.client_data_hash),
                    algorithms: request.algorithms,
                    exclude_credential_ids: request
                        .exclude_credential_ids
                        .iter()
                        .map(|id| encode(id))
                        .collect(),
                    password_session: None,
                }),
            )
            .await?;
        let OperationSuccess::RegisterPasskey(result) = result else {
            return Err(CtapStatus::Other.into());
        };
        response::make_credential(&decode(&result.authenticator_data)?)
    }

    async fn get_assertion(
        &mut self,
        request: GetAssertionRequest,
        progress: &Progress,
    ) -> Result<Vec<u8>, CtapError> {
        let operation = Operation::AssertPasskey(AssertPasskeyArgs {
            rp_id: request.rp_id,
            client_data_hash: encode(&request.client_data_hash),
            allow_credential_ids: request
                .allow_credential_ids
                .iter()
                .map(|id| encode(id))
                .collect(),
            user_present: request.user_presence,
            password_session: None,
        });
        let result = if request.user_presence {
            self.request_interactive(progress, operation).await?
        } else {
            self.request(operation).await?
        };
        let OperationSuccess::AssertPasskey(result) = result else {
            return Err(CtapStatus::Other.into());
        };
        response::get_assertion(&Assertion {
            credential_id: &decode(&result.credential_id)?,
            authenticator_data: &decode(&result.authenticator_data)?,
            signature: &decode(&result.signature)?,
            user_id: &decode(&result.user_handle)?,
            user_name: result.user_name.as_deref(),
            user_selected: result.user_selected,
        })
    }

    async fn request_interactive(
        &mut self,
        progress: &Progress,
        operation: Operation,
    ) -> Result<OperationSuccess, CtapError> {
        progress.waiting_for_user(true);
        let result = self.request(operation).await;
        progress.waiting_for_user(false);
        result
    }

    async fn request(&mut self, operation: Operation) -> Result<OperationSuccess, CtapError> {
        self.session.request(operation).await.map_err(client_error)
    }
}

/// Map a failure from the desktop app onto the status a platform expects.
fn client_error(error: ClientError) -> CtapError {
    let ClientError::Operation { code, .. } = &error else {
        // The app is closed, was never paired, or dropped the frame: from the
        // platform's side the authenticator is simply unavailable.
        return CtapStatus::Other.into();
    };
    match code.as_str() {
        "passkey_excluded" => CtapStatus::CredentialExcluded,
        "passkey_unsupported_algorithm" => CtapStatus::UnsupportedAlgorithm,
        "passkey_not_found" => CtapStatus::NoCredentials,
        "passkey_consent_denied" | "password_required" | "invalid_credentials" => {
            CtapStatus::OperationDenied
        }
        "database_locked" | "database_not_selected" | "database_not_found" => {
            CtapStatus::OperationDenied
        }
        "invalid_passkey_request" => CtapStatus::InvalidParameter,
        _ => CtapStatus::Other,
    }
    .into()
}

fn encode(value: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(value)
}

fn decode(value: &str) -> Result<Vec<u8>, CtapError> {
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| CtapError::new(CtapStatus::Other))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_app_failures_onto_platform_visible_statuses() {
        let operation = |code: &str| ClientError::Operation {
            code: code.into(),
            message: String::new(),
        };
        assert_eq!(
            client_error(operation("database_locked")).status,
            CtapStatus::OperationDenied
        );
        assert_eq!(
            client_error(operation("passkey_excluded")).status,
            CtapStatus::CredentialExcluded
        );
        assert_eq!(
            client_error(operation("passkey_not_found")).status,
            CtapStatus::NoCredentials
        );
        assert_eq!(
            client_error(operation("passkey_unsupported_algorithm")).status,
            CtapStatus::UnsupportedAlgorithm
        );
        assert_eq!(
            client_error(operation("something_new")).status,
            CtapStatus::Other
        );
        assert_eq!(
            client_error(ClientError::Rejected).status,
            CtapStatus::Other
        );
    }

    #[test]
    fn progress_reports_user_presence_only_while_waiting() {
        let progress = Progress::default();
        assert_eq!(progress.status(), KeepaliveStatus::Processing);
        progress.waiting_for_user(true);
        assert_eq!(progress.status(), KeepaliveStatus::UserPresenceNeeded);
        progress.waiting_for_user(false);
        assert_eq!(progress.status(), KeepaliveStatus::Processing);
    }

    #[test]
    fn advertises_the_same_aaguid_the_credentials_carry() {
        assert_eq!(KEELESS_AAGUID, keeless_kdbx::KEELESS_AAGUID);
    }
}
