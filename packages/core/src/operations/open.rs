use keeless_schema::{EmptyResult, OpenArgs, OpenTarget, OperationSuccess};

use crate::{CoreError, DatabaseId, KeelessCore, Result, Selection, StorageDescriptor};

pub(crate) async fn run(core: &mut KeelessCore, descriptor: StorageDescriptor) -> Result<()> {
    let storage = core
        .storage_providers
        .get(&descriptor.provider)
        .cloned()
        .ok_or_else(|| CoreError::UnknownStorageProvider(descriptor.provider.clone()))?;
    let normalized_path = storage.get_normalized_path(&descriptor.path)?;
    let database_id = DatabaseId::from_storage(&descriptor.provider, &normalized_path)?;
    // Persistence selection is process-global in desktop hosts. Drop the old handle and
    // selection before changing namespaces so a failed open cannot journal the old DB elsewhere.
    super::lock::run(core);
    core.selection = None;
    core.persistence.select(&database_id).await?;
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
    let (exists, mut sync_error) = match storage.provider().stat(&descriptor.path).await {
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
        descriptor: Some(descriptor),
        storage: Some(storage),
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

async fn run_recent(core: &mut KeelessCore, id: String) -> Result<()> {
    let database_id = DatabaseId::from_recent_id(&id)?;
    if !super::recent::contains(core, &id).await? {
        return Err(CoreError::InvalidRecentDatabase);
    }
    super::lock::run(core);
    core.selection = None;
    core.persistence.select(&database_id).await?;
    core.selection = Some(Selection {
        descriptor: None,
        storage: None,
        database_id,
        exists: true,
    });
    core.sync_status = crate::SyncStatus::Idle;
    core.sync_error = None;
    core.dirty = false;
    Ok(())
}

pub(super) async fn execute(core: &mut KeelessCore, args: OpenArgs) -> Result<OperationSuccess> {
    match args.target {
        OpenTarget::Storage { storage } => run(core, storage).await?,
        OpenTarget::Database { database_id } => run_recent(core, database_id).await?,
    }
    Ok(OperationSuccess::Open(EmptyResult {}))
}
