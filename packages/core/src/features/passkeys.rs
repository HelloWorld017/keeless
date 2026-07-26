//! Shared plumbing for the CTAP-facing passkey operations.
//!
//! The operations here are deliberately UI-free: user presence and verification
//! are the transport's job (a virtual HID daemon on Linux, Windows Hello behind
//! the plugin authenticator), so `userVerified` arrives as a caller assertion and
//! is only translated into the authenticator-data flag.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use keeless_kdbx::{CompositeKey, NodeId, PasskeyCredential, PasskeyError, UserVerification};
use keeless_schema::PasskeySummary;
use std::collections::HashSet;
use zeroize::Zeroizing;

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

/// A composite key plus the password it came from, when the host had to prompt.
///
/// Carrying the password lets a nested operation reuse it, so paranoia mode asks
/// once per passkey ceremony instead of once per underlying mutation.
pub(crate) struct UnlockedKey {
    pub(crate) key: CompositeKey,
    pub(crate) password: Option<Zeroizing<Vec<u8>>>,
}

impl UnlockedKey {
    pub(crate) fn password(&self) -> Option<&[u8]> {
        self.password.as_ref().map(|password| password.as_slice())
    }
}

/// Obtain the composite key needed to read protected passkey fields.
///
/// Mirrors the reveal/update operations: the cached credential is preferred, and
/// paranoia mode falls back to prompting the host.
pub(crate) async fn unlock(core: &mut KeelessCore, mode: PasswordInputMode) -> Result<UnlockedKey> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    if let Some(credential) = &core.credential {
        return Ok(UnlockedKey {
            key: credential.restore_key()?,
            password: None,
        });
    }
    let password = core.request_password(mode).await?;
    let key = CompositeKey::new().with_password(&password)?;
    core.handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .verify_credentials(&key)?;
    Ok(UnlockedKey {
        key,
        password: Some(password),
    })
}

/// Credentials reachable from the group tree, excluding trashed entries.
pub(crate) fn visible_credentials(
    core: &KeelessCore,
    key: &CompositeKey,
    rp_id: Option<&str>,
) -> Result<Vec<(NodeId, PasskeyCredential)>> {
    let database = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database();
    let visible = all_entries(database, true)
        .into_iter()
        .map(|entry| entry.id)
        .collect::<HashSet<_>>();
    let mut credentials = keeless_kdbx::find_credentials(database, key, rp_id)?;
    credentials.retain(|(id, _)| visible.contains(id));
    Ok(credentials)
}

pub(crate) fn summary(entry_id: NodeId, credential: &PasskeyCredential) -> PasskeySummary {
    PasskeySummary {
        entry_id: node_id(entry_id),
        credential_id: encode(credential.credential_id()),
        rp_id: credential.rp_id().to_string(),
        username: credential.username().to_string(),
        user_handle: encode(credential.user_handle()),
        algorithm: credential.algorithm().cose_id(),
    }
}
