use keeless_schema::{GetPasskeysMetadataArgs, OperationSuccess, PasskeysMetadataResult};

use crate::features::passkeys;
use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn run(
    core: &mut KeelessCore,
    args: GetPasskeysMetadataArgs,
) -> Result<PasskeysMetadataResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let allowed_credential_ids = args
        .allow_credential_ids
        .iter()
        .map(|value| passkeys::decode(value))
        .collect::<Result<Vec<_>>>()?;
    let credentials =
        passkeys::visible_credentials(core, args.rp_id.as_deref(), &allowed_credential_ids)?
            .into_iter()
            .map(|(entry_id, credential)| passkeys::summary(entry_id, &credential))
            .collect();
    Ok(PasskeysMetadataResult { credentials })
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: GetPasskeysMetadataArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetPasskeysMetadata(
        run(core, args).await?,
    ))
}
