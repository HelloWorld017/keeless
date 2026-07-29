use keeless_kdbx::save_database;
use keeless_schema::{OperationSuccess, PrepareDatabaseExportArgs, PrepareDatabaseExportResult};
use zeroize::Zeroizing;

use crate::{KeelessCore, Result};

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: PrepareDatabaseExportArgs,
) -> Result<OperationSuccess> {
    let password = args
        .password
        .take()
        .map(|password| Zeroizing::new(password.into_bytes()));
    let key = core
        .current_key(password.as_deref().map(Vec::as_slice))
        .await?;
    let bytes = {
        let database = core
            .handle
            .as_ref()
            .ok_or(crate::CoreError::DatabaseLocked)?
            .database();
        let mut bytes = Zeroizing::new(Vec::new());
        save_database(&mut *bytes, database, &key)?;
        bytes
    };
    let transfer_id = core.publish_download_transfer(bytes)?;
    Ok(OperationSuccess::PrepareDatabaseExport(
        PrepareDatabaseExportResult { transfer_id },
    ))
}
