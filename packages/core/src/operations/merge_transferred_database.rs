use keeless_kdbx::{
    CompositeCredentials, DatabaseMerger, MergeStrategy, open_database, reencrypt_memory_protection,
};
use keeless_schema::{
    MergeTransferredDatabaseArgs, MergeTransferredDatabaseResult, OperationSuccess,
};

use crate::{
    CoreError, KeelessCore, Result, extensions::password_session::PasswordSessionExtension,
};

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: MergeTransferredDatabaseArgs,
) -> Result<OperationSuccess> {
    if core.sync_key_migration_pending() {
        return Err(CoreError::SyncRecoveryRequired);
    }
    let source_password =
        zeroize::Zeroizing::new(std::mem::take(&mut args.source_password).into_bytes());
    let target_password = {
        let now_millis = core.clock.monotonic_millis();
        core.extensions
            .get_mut::<PasswordSessionExtension>()
            .resolve_argument(
                args.password.take(),
                args.password_session.take(),
                now_millis,
            )?
    };
    let target_key = core
        .current_key(target_password.as_ref().map(|password| password.as_slice()))
        .await?;
    let bytes = core.consume_upload_transfer(&args.transfer_id)?;
    let source_credentials = CompositeCredentials::new().with_password(&source_password)?;
    let opened =
        open_database(bytes.as_slice(), &source_credentials).map_err(|error| match error {
            keeless_kdbx::DatabaseError::InvalidCredentials => CoreError::InvalidSourceCredentials,
            error => error.into(),
        })?;

    // Protected values are memory-encrypted with the source key. Re-encrypt the parsed
    // source under the target key before the credential-aware merger compares either side.
    let mut source = opened.database;
    reencrypt_memory_protection(&mut source, &opened.key, &target_key)?;
    let result = {
        let target = core
            .handle
            .as_mut()
            .ok_or(CoreError::DatabaseLocked)?
            .database_mut();
        DatabaseMerger::with_credentials(MergeStrategy::NewestWins, &target_key)
            .merge(target, &source)
    };
    let sync_error = core
        .sync_with_key(&target_key, None)
        .await
        .err()
        .map(|error| (&error).into());

    Ok(OperationSuccess::MergeTransferredDatabase(
        MergeTransferredDatabaseResult {
            entries_added: result.entries_added as u64,
            entries_modified: result.entries_modified as u64,
            entries_deleted: result.entries_deleted as u64,
            groups_added: result.groups_added as u64,
            groups_modified: result.groups_modified as u64,
            groups_deleted: result.groups_deleted as u64,
            conflict_count: result.conflicts.len() as u64,
            sync_error,
        },
    ))
}
