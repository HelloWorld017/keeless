use keeless_kdbx::{CtapAuthenticationRequest, PasskeyCredential};
use keeless_schema::{AssertPasskeyArgs, AssertPasskeyResult, OperationSuccess};

use crate::features::passkeys;
use crate::{
    CoreError, KeelessCore, PasskeyConsentMode, PasskeyConsentRequest, PasswordInputMode, Result,
};

/// Matches the native dialog's bounded account picker.
const MAX_CONSENT_ACCOUNTS: usize = 32;

pub(crate) async fn run(
    core: &mut KeelessCore,
    args: AssertPasskeyArgs,
) -> Result<AssertPasskeyResult> {
    if args.user_present {
        passkeys::ensure_database_unlocked(core).await?;
    } else if core.handle.is_none() || core.credential.is_none() {
        // A silent assertion may not make a user enter their password, including
        // in paranoia mode where the database remains open but no key is cached.
        return Err(CoreError::PasskeyNotFound);
    }
    let client_data_hash = passkeys::decode(&args.client_data_hash)?;
    let allowed_credential_ids = args
        .allow_credential_ids
        .iter()
        .map(|value| passkeys::decode(value))
        .collect::<Result<Vec<_>>>()?;
    if !args.user_present && allowed_credential_ids.is_empty() {
        return Err(CoreError::PasskeyNotFound);
    }
    let mut candidates =
        passkeys::visible_credentials(core, Some(&args.rp_id), &allowed_credential_ids)?;
    if candidates.is_empty() {
        return Err(CoreError::PasskeyNotFound);
    }

    let (entry_id, username, user_selected) = if args.user_present {
        let user_selected = candidates.len() > 1;
        candidates.truncate(MAX_CONSENT_ACCOUNTS);
        let accounts = candidates
            .iter()
            .map(|(_, credential)| credential.username.clone())
            .collect::<Vec<_>>();
        let selected = core
            .request_passkey_consent(PasskeyConsentRequest {
                mode: PasskeyConsentMode::Assert,
                rp_id: args.rp_id.clone(),
                accounts,
            })
            .await?;
        let (entry_id, credential) = candidates
            .into_iter()
            .nth(selected)
            .ok_or(CoreError::PasskeyConsentDenied)?;
        (entry_id, Some(credential.username), user_selected)
    } else {
        let (entry_id, _) = candidates.remove(0);
        (entry_id, None, false)
    };

    let key = passkeys::unlock(core, PasswordInputMode::Reveal).await?;

    let database = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database();
    let credential = PasskeyCredential::from_database_entry(database, &key, &entry_id)
        .map_err(CoreError::from)?
        .ok_or(CoreError::PasskeyNotFound)?;

    let response = credential.authenticate_ctap(&CtapAuthenticationRequest {
        client_data_hash: &client_data_hash,
        rp_id: &args.rp_id,
        allowed_credential_ids: &[],
        user_presence: passkeys::user_presence(args.user_present),
        user_verification: passkeys::user_verification(args.user_present),
    })?;

    // Auto-lock measures the user's activity, and a silent assertion is by
    // definition not that: it exists so a platform can look before it asks
    // anyone anything, so it must not keep the database open.
    if args.user_present {
        core.touch_activity();
    }
    Ok(AssertPasskeyResult {
        credential_id: passkeys::encode(&response.credential_id),
        authenticator_data: passkeys::encode(&response.authenticator_data),
        signature: passkeys::encode(&response.signature),
        user_handle: passkeys::encode(&response.user_handle),
        user_name: username,
        user_selected,
    })
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: AssertPasskeyArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::AssertPasskey(run(core, args).await?))
}
