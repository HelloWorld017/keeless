//! Shared plumbing for Core-owned CTAP-facing passkey operations.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use keeless_kdbx::{
    CompositeKey, NodeId, PasskeyCredentialSummary, PasskeyError, UserPresence, UserVerification,
};
use keeless_schema::PasskeySummary;
use std::collections::HashSet;

use crate::model::{all_entries, node_id};
use crate::{CoreError, KeelessCore, PasswordInputMode, Result};

/// Decode a base64url value carried by the passkey operations.
pub(crate) fn decode(value: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| CoreError::InvalidPasskeyRequest(PasskeyError::InvalidClientData))
}

pub(crate) fn encode(value: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(value)
}

pub(crate) fn user_verification(user_verified: bool) -> UserVerification {
    if user_verified {
        UserVerification::Verified
    } else {
        UserVerification::NotVerified
    }
}

pub(crate) fn user_presence(user_present: bool) -> UserPresence {
    if user_present {
        UserPresence::Present
    } else {
        UserPresence::NotPresent
    }
}

/// Obtain the composite key needed to read protected passkey fields.
///
/// Mirrors the reveal/update operations: the cached credential is preferred, and
/// paranoia mode falls back to prompting the host.
pub(crate) async fn unlock(
    core: &mut KeelessCore,
    mode: PasswordInputMode,
) -> Result<CompositeKey> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    if let Some(credential) = &core.credential {
        return credential.restore_key();
    }
    let password = core.request_password(mode).await?;
    let key = CompositeKey::new().with_password(&password)?;
    core.handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .verify_credentials(&key)?;
    Ok(key)
}

/// Open the selected database for an interactive passkey ceremony.
///
/// A silent assertion deliberately never uses this path: a page can issue one
/// without user interaction, so it must not cause a password prompt.
pub(crate) async fn ensure_database_unlocked(core: &mut KeelessCore) -> Result<()> {
    match crate::operations::get_database_status::run(core) {
        crate::DatabaseStatus::Unlocked => Ok(()),
        crate::DatabaseStatus::NotExist => Err(CoreError::PasskeyNotFound),
        crate::DatabaseStatus::Locked => {
            let password = core.request_password(PasswordInputMode::Unlock).await?;
            crate::operations::unlock::run(core, &password).await
        }
    }
}

/// Credentials reachable from the group tree, excluding trashed entries.
pub(crate) fn visible_credentials(
    core: &KeelessCore,
    rp_id: Option<&str>,
    allowed_credential_ids: &[Vec<u8>],
) -> Result<Vec<(NodeId, PasskeyCredentialSummary)>> {
    let database = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database();
    let visible = all_entries(database, true)
        .into_iter()
        .map(|entry| entry.id)
        .collect::<HashSet<_>>();
    let mut credentials = keeless_kdbx::find_passkey_credentials(database, rp_id)?;
    credentials.retain(|(id, _)| visible.contains(id));
    if !allowed_credential_ids.is_empty() {
        let extension = core
            .extensions
            .get::<crate::extensions::passkey::PasskeyExtension>();
        credentials.retain(|(id, _)| extension.matches_any(id, allowed_credential_ids));
    }
    Ok(credentials)
}

pub(crate) fn summary(entry_id: NodeId, credential: &PasskeyCredentialSummary) -> PasskeySummary {
    PasskeySummary {
        entry_id: node_id(entry_id),
        rp_id: credential.rp_id.clone(),
        username: credential.username.clone(),
    }
}
