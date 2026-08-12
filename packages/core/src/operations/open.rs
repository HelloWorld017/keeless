use keeless_schema::{EmptyResult, OpenArgs, OperationSuccess};

use crate::{CoreError, KeelessCore, Result, Selection, StorageDescriptor};

pub(crate) async fn run(core: &mut KeelessCore, descriptor: StorageDescriptor) -> Result<()> {
    let provider = core
        .storage_providers
        .get(&descriptor.provider)
        .cloned()
        .ok_or_else(|| CoreError::UnknownStorageProvider(descriptor.provider.clone()))?;
    // Persistence selection is process-global in desktop hosts. Drop the old handle and
    // selection before changing namespaces so a failed open cannot journal the old DB elsewhere.
    super::lock::run(core);
    core.selection = None;
    let database_id = core.persistence.select(&descriptor).await?;
    let (cache_exists, journal_dirty, persistence_error) = {
        let (cache_exists, mut error) = match core.persistence.read_cache().await {
            Ok(cache) => (cache.is_some(), None),
            Err(cache_error) => {
                core.persistence
                    .quarantine_cache(&cache_error.to_string())
                    .await?;
                (false, Some(cache_error))
            }
        };
        let journal_dirty = match core.persistence.read_journal().await {
            Ok(lines) => !lines.is_empty(),
            Err(journal_error) => {
                if error.is_none() {
                    error = Some(journal_error);
                }
                // Defer quarantine until unlock has successfully opened the source DB.
                true
            }
        };
        (cache_exists, journal_dirty, error)
    };
    let (exists, mut sync_error) = match provider.stat(&descriptor.path).await {
        Ok(metadata) => (metadata.is_some() || cache_exists, None),
        Err(error) if cache_exists => {
            let error = CoreError::from(error);
            (true, Some((&error).into()))
        }
        Err(error) => return Err(error.into()),
    };
    if sync_error.is_none() {
        sync_error = persistence_error.as_ref().map(Into::into);
    }
    core.selection = Some(Selection {
        descriptor,
        provider,
        database_id,
        exists,
    });
    core.sync_status = if sync_error.is_some() {
        crate::SyncStatus::Error
    } else {
        crate::SyncStatus::Idle
    };
    core.sync_error = sync_error;
    core.dirty = journal_dirty;
    Ok(())
}

pub(super) async fn execute(core: &mut KeelessCore, args: OpenArgs) -> Result<OperationSuccess> {
    run(core, args.storage).await?;
    Ok(OperationSuccess::Open(EmptyResult {}))
}
