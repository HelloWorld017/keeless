use keeless_kdbx::{CtapAuthenticationRequest, PasskeyCredential};
use keeless_schema::{AssertPasskeyArgs, AssertPasskeyResult, OperationSuccess};

use crate::features::passkeys;
use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, PasswordInputMode, Result};

pub(crate) async fn run(
    core: &mut KeelessCore,
    args: AssertPasskeyArgs,
) -> Result<AssertPasskeyResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let entry_id = parse_node_id(args.entry_id)?;
    let client_data_hash = passkeys::decode(&args.client_data_hash)?;
    let unlocked = passkeys::unlock(core, PasswordInputMode::Reveal).await?;

    let database = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database();
    let credential = PasskeyCredential::from_database_entry(database, &unlocked.key, &entry_id)
        .map_err(CoreError::from)?
        .ok_or(CoreError::PasskeyNotFound)?;

    let response = credential.authenticate_ctap(&CtapAuthenticationRequest {
        client_data_hash: &client_data_hash,
        rp_id: &args.rp_id,
        allowed_credential_ids: &[],
        user_verification: passkeys::user_verification(args.user_verified),
    })?;

    core.touch_activity();
    Ok(AssertPasskeyResult {
        credential_id: passkeys::encode(&response.credential_id),
        authenticator_data: passkeys::encode(&response.authenticator_data),
        signature: passkeys::encode(&response.signature),
        user_handle: passkeys::encode(&response.user_handle),
    })
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: AssertPasskeyArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::AssertPasskey(run(core, args).await?))
}
