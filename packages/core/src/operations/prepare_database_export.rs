use keeless_kdbx::save_database;
use keeless_schema::{OperationSuccess, PrepareDatabaseExportArgs, PrepareDatabaseExportResult};
use zeroize::Zeroizing;

use crate::{KeelessCore, Result, extensions::password_session::PasswordSessionExtension};

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: PrepareDatabaseExportArgs,
) -> Result<OperationSuccess> {
    let password = {
        let now_millis = core.clock.monotonic_millis();
        core.extensions
            .get_mut::<PasswordSessionExtension>()
            .resolve_argument(
                args.password.take(),
                args.password_session.take(),
                now_millis,
            )?
    };
    let key = core
        .current_key(password.as_ref().map(|password| password.as_slice()))
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
