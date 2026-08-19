use keeless_schema::{GetPasskeysArgs, OperationSuccess, PasskeyCredentialInfo, PasskeysResult};

use crate::features::passkeys;
use crate::model::node_id;
use crate::{CoreError, KeelessCore, Result};

/// Return the protected metadata Windows needs for browser credential discovery.
///
/// The composite key is resolved once before any entry is visited. Private keys
/// are parsed only inside KDBX and never cross the operation boundary.
pub(crate) async fn run(core: &mut KeelessCore, args: GetPasskeysArgs) -> Result<PasskeysResult> {
    let key = passkeys::unlock_without_prompt(core, args.password, args.password_session).await?;
    let candidates = passkeys::visible_credentials(core, None, &[])?;
    let database = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database();
    let mut credentials = Vec::with_capacity(candidates.len());

    for (entry_id, summary) in candidates {
        let entry = database
            .get_entry(&entry_id)
            .ok_or(CoreError::EntryNotFound)?;
        let credential = keeless_kdbx::PasskeyCredential::metadata_from_database_entry(
            database, &key, &entry_id,
        )?
        .ok_or(CoreError::PasskeyNotFound)?;
        let title = if !entry.title().is_protected() {
            entry.title().as_str().trim()
        } else {
            ""
        };
        let rp_name = if title.is_empty() {
            summary.rp_id.clone()
        } else {
            title.to_owned()
        };
        let user_name = credential.username.clone();

        credentials.push(PasskeyCredentialInfo {
            entry_id: node_id(entry_id),
            credential_id: passkeys::encode(&credential.credential_id),
            rp_id: credential.rp_id,
            rp_name,
            user_id: passkeys::encode(&credential.user_handle),
            user_display_name: user_name.clone(),
            user_name,
        });
    }

    Ok(PasskeysResult { credentials })
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: GetPasskeysArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetPasskeys(run(core, args).await?))
}
