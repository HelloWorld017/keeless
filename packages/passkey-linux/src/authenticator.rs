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
use keeless_schema::{
    AssertPasskeyArgs, DatabaseStatus, GetDatabaseStatusArgs, GetPasskeysArgs, Operation,
    OperationSuccess, PasskeySummary, RegisterPasskeyArgs, UnlockArgs,
};

use crate::consent::{Account, Ceremony, ConsentError, ConsentPrompt};
use crate::ctaphid::KeepaliveStatus;
use crate::session::Session;

/// Most accounts the consent prompt will list, matching what its dialog accepts.
const MAX_PROMPT_ACCOUNTS: usize = 32;

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
    consent: ConsentPrompt,
    /// Transports to advertise in `authenticatorGetInfo`.
    transports: &'static [&'static str],
}

impl Authenticator {
    pub fn new(session: Session, consent: ConsentPrompt) -> Self {
        Self {
            session,
            consent,
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
            // Selection asks whether the user wants to use this authenticator at
            // all, which is exactly the question the consent prompt answers.
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
        self.confirm(
            progress,
            Ceremony::Assert,
            "this device",
            &[Account::new("selection".into(), "Use Keeless")],
        )
        .await?;
        Ok(response::status(CtapStatus::Success))
    }

    async fn make_credential(
        &mut self,
        request: MakeCredentialRequest,
        progress: &Progress,
    ) -> Result<Vec<u8>, CtapError> {
        // Refused before the prompt, so the user is never asked to approve a
        // ceremony that cannot succeed.
        if !request
            .algorithms
            .iter()
            .any(|algorithm| response::SUPPORTED_ALGORITHMS.contains(algorithm))
        {
            return Err(CtapStatus::UnsupportedAlgorithm.into());
        }
        self.ensure_unlocked(progress).await?;
        let exclude_credential_ids = request
            .exclude_credential_ids
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>();
        let existing = self
            .passkeys(Some(&request.rp_id), &exclude_credential_ids)
            .await?;

        // A credential the relying party excluded still needs the user's consent
        // before being told one exists, so the answer cannot be used to probe.
        if !existing.is_empty() {
            // The answer is the same whether or not the user approves, so a
            // decline cannot be used to tell "excluded" apart from "denied".
            let _ = self
                .confirm(
                    progress,
                    Ceremony::Register,
                    &request.rp_id,
                    &[Account::new("excluded".into(), &request.user_name)],
                )
                .await;
            return Err(CtapStatus::CredentialExcluded.into());
        }

        self.confirm(
            progress,
            Ceremony::Register,
            &request.rp_id,
            &[Account::new("register".into(), &request.user_name)],
        )
        .await?;

        let result = self
            .request(Operation::RegisterPasskey(RegisterPasskeyArgs {
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
                user_verified: true,
            }))
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
        if request.user_presence {
            self.ensure_unlocked(progress).await?;
        } else {
            // A silent request exists to confirm credentials a platform already
            // holds identifiers for. Answering one with no allow list would let
            // any process that can reach the device enumerate every site the
            // user has an account at, without anyone approving anything.
            if request.allow_credential_ids.is_empty() {
                return Err(CtapStatus::NoCredentials.into());
            }
            // Nor may a silent request raise the master-password prompt: a page
            // can send one on load, and a prompt nobody asked for is exactly
            // what a fake prompt needs the user to be used to.
            if !self.is_unlocked().await? {
                return Err(CtapStatus::NoCredentials.into());
            }
        }
        let allowed_credential_ids = request
            .allow_credential_ids
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>();
        let mut candidates = self
            .passkeys(Some(&request.rp_id), &allowed_credential_ids)
            .await?;
        if candidates.is_empty() {
            return Err(CtapStatus::NoCredentials.into());
        }

        // A silent assertion answers "does a credential exist" without asking
        // anyone. Its user presence flag is clear, so a relying party rejects it.
        let (chosen, user_selected) = if request.user_presence {
            let accounts = prompt_accounts(&candidates);
            let chosen = self
                .confirm(progress, Ceremony::Assert, &request.rp_id, &accounts)
                .await?;
            let index = accounts
                .iter()
                .position(|account| account.id == chosen)
                .ok_or(CtapError::new(CtapStatus::Other))?;
            (candidates[index].clone(), candidates.len() > 1)
        } else {
            (candidates.remove(0), false)
        };

        let result = self
            .request(Operation::AssertPasskey(AssertPasskeyArgs {
                entry_id: chosen.entry_id,
                rp_id: request.rp_id,
                client_data_hash: encode(&request.client_data_hash),
                user_present: request.user_presence,
                user_verified: request.user_presence,
            }))
            .await?;
        let OperationSuccess::AssertPasskey(result) = result else {
            return Err(CtapStatus::Other.into());
        };
        response::get_assertion(&Assertion {
            credential_id: &decode(&result.credential_id)?,
            authenticator_data: &decode(&result.authenticator_data)?,
            signature: &decode(&result.signature)?,
            user_id: &decode(&result.user_handle)?,
            // A name is only meaningful once someone has approved seeing it.
            user_name: request.user_presence.then_some(chosen.username.as_str()),
            user_selected,
        })
    }

    /// Make sure the database is open before a ceremony needs it.
    ///
    /// Asks for the status rather than reading credentials: the status needs no
    /// key, so it neither decrypts the database nor triggers the password prompt
    /// that paranoia mode puts in front of every read.
    ///
    /// The unlock prompt belongs to the app, not this daemon, so the transport is
    /// told to expect the user to be busy for a while.
    async fn ensure_unlocked(&mut self, progress: &Progress) -> Result<(), CtapError> {
        match self.database_status().await? {
            DatabaseStatus::Unlocked => Ok(()),
            // With no database selected there is nothing to unlock, and no
            // prompt would help.
            DatabaseStatus::NotExist => Err(CtapStatus::NoCredentials.into()),
            DatabaseStatus::Locked => {
                progress.waiting_for_user(true);
                let unlocked = self
                    .request(Operation::Unlock(UnlockArgs { password: None }))
                    .await;
                progress.waiting_for_user(false);
                unlocked.map(|_| ())
            }
        }
    }

    async fn is_unlocked(&mut self) -> Result<bool, CtapError> {
        Ok(self.database_status().await? == DatabaseStatus::Unlocked)
    }

    async fn database_status(&mut self) -> Result<DatabaseStatus, CtapError> {
        let status = self
            .request(Operation::GetDatabaseStatus(GetDatabaseStatusArgs {}))
            .await?;
        let OperationSuccess::GetDatabaseStatus(status) = status else {
            return Err(CtapStatus::Other.into());
        };
        Ok(status.status)
    }

    async fn passkeys(
        &mut self,
        rp_id: Option<&str>,
        allowed_credential_ids: &[&[u8]],
    ) -> Result<Vec<PasskeySummary>, CtapError> {
        let result = self
            .request(Operation::GetPasskeys(GetPasskeysArgs {
                rp_id: rp_id.map(str::to_owned),
                allow_credential_ids: allowed_credential_ids.iter().map(|id| encode(id)).collect(),
            }))
            .await?;
        let OperationSuccess::GetPasskeys(result) = result else {
            return Err(CtapStatus::Other.into());
        };
        Ok(result.credentials)
    }

    /// Show the consent prompt and return the chosen account's ID.
    async fn confirm(
        &mut self,
        progress: &Progress,
        ceremony: Ceremony,
        rp_id: &str,
        accounts: &[Account],
    ) -> Result<String, CtapError> {
        progress.waiting_for_user(true);
        let chosen = self.consent.request(ceremony, rp_id, accounts).await;
        progress.waiting_for_user(false);
        match chosen {
            Ok(Some(id)) => Ok(id),
            // A prompt the user closed and one that timed out both mean the
            // ceremony was not authorized.
            Ok(None) | Err(ConsentError::TimedOut) => Err(CtapStatus::OperationDenied.into()),
            Err(ConsentError::Failed(_)) => Err(CtapStatus::Other.into()),
        }
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
        // No database is open, it is locked, or the user dismissed its password
        // prompt — all of which mean the user did not authorize this ceremony.
        "database_locked"
        | "database_not_selected"
        | "database_not_found"
        | "password_required"
        | "invalid_credentials" => CtapStatus::OperationDenied,
        "invalid_passkey_request" => CtapStatus::InvalidParameter,
        _ => CtapStatus::Other,
    }
    .into()
}

/// Labels for the picker, one per candidate.
///
/// Capped at what the prompt will display, so a database with an unusual number
/// of credentials for one relying party still produces a usable dialog.
fn prompt_accounts(candidates: &[PasskeySummary]) -> Vec<Account> {
    candidates
        .iter()
        .take(MAX_PROMPT_ACCOUNTS)
        .enumerate()
        .map(|(index, credential)| Account::new(index.to_string(), &credential.username))
        .collect()
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
