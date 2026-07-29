use keeless_kdbx::{
    CompositeKey, DatabaseMerger, MergeStrategy, open_database, save_database_with_credentials,
};
use keeless_schema::{
    MergeTransferredDatabaseArgs, MergeTransferredDatabaseResult, OperationSuccess,
};
use zeroize::Zeroizing;

use crate::{CoreError, KeelessCore, Result};

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: MergeTransferredDatabaseArgs,
) -> Result<OperationSuccess> {
    let bytes = core.consume_upload_transfer(&args.transfer_id)?;
    let source_password = Zeroizing::new(std::mem::take(&mut args.source_password).into_bytes());
    let target_password = args
        .password
        .take()
        .map(|password| Zeroizing::new(password.into_bytes()));
    let target_key = core
        .current_key(target_password.as_deref().map(Vec::as_slice))
        .await?;
    let source_key = CompositeKey::new().with_password(&source_password)?;
    let source = open_database(bytes.as_slice(), &source_key)?;

    // Protected values are memory-encrypted with the source key. Re-encrypt the parsed
    // source under the target key before the credential-aware merger compares either side.
    let source = {
        let mut rekeyed = Zeroizing::new(Vec::new());
        save_database_with_credentials(&mut *rekeyed, &source, &source_key, &target_key)?;
        open_database(rekeyed.as_slice(), &target_key)?
    };
    let result = {
        let target = core
            .handle
            .as_mut()
            .ok_or(CoreError::DatabaseLocked)?
            .database_mut();
        DatabaseMerger::with_credentials(MergeStrategy::NewestWins, &target_key)
            .merge(target, &source)
    };
    core.sync_with_key(target_key, None).await?;

    Ok(OperationSuccess::MergeTransferredDatabase(
        MergeTransferredDatabaseResult {
            entries_added: result.entries_added as u64,
            entries_modified: result.entries_modified as u64,
            entries_deleted: result.entries_deleted as u64,
            groups_added: result.groups_added as u64,
            groups_modified: result.groups_modified as u64,
            groups_deleted: result.groups_deleted as u64,
            conflict_count: result.conflicts.len() as u64,
        },
    ))
}
