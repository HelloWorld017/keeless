use keeless_schema::{EmptyResult, OpenArgs, OpenTarget, OperationSuccess};

use crate::{CoreError, DatabaseId, KeelessCore, Result, Selection};

pub(crate) async fn run(core: &mut KeelessCore, target: OpenTarget) -> Result<()> {
    let (descriptor, storage, database_id) = match target {
        OpenTarget::Storage {
            storage: descriptor,
        } => {
            let storage = core
                .storage_providers
                .get(&descriptor.provider)
                .cloned()
                .ok_or_else(|| CoreError::UnknownStorageProvider(descriptor.provider.clone()))?;
            let normalized_path = storage.get_normalized_path(&descriptor.path)?;
            let database_id = DatabaseId::from_storage(&descriptor.provider, &normalized_path)?;
            (Some(descriptor), Some(storage), database_id)
        }
        OpenTarget::Database { database_id: id } => {
            let database_id = DatabaseId::from_recent_id(&id)?;
            let record = crate::recent::load(core)
                .await?
                .databases
                .into_iter()
                .find(|database| database.id == id)
                .ok_or(CoreError::InvalidRecentDatabase)?;
            let descriptor = record.descriptor;
            let storage = core
                .storage_providers
                .get(&descriptor.provider)
                .cloned()
                .ok_or_else(|| CoreError::UnknownStorageProvider(descriptor.provider.clone()))?;
            let normalized_path = storage.get_normalized_path(&descriptor.path)?;
            if DatabaseId::from_storage(&descriptor.provider, &normalized_path)? != database_id {
                return Err(CoreError::InvalidRecentDatabase);
            }
            (Some(descriptor), Some(storage), database_id)
        }
    };
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
    let (exists, mut sync_error) = match (&storage, &descriptor) {
        (Some(storage), Some(descriptor)) => {
            match storage.provider().stat(&descriptor.path).await {
                Ok(metadata) => (metadata.is_some() || cache_exists, None),
                Err(error) if cache_exists => {
                    let error = CoreError::from(error);
                    (true, Some((&error).into()))
                }
                Err(error) => return Err(error.into()),
            }
        }
        (None, None) => (true, None),
        _ => unreachable!("storage and descriptor are resolved together"),
    };
    if sync_error.is_none() {
        sync_error = persistence_error.as_ref().map(Into::into);
    }
    core.selection = Some(Selection {
        descriptor,
        storage,
        database_id,
        exists,
    });
    let sync_status = if sync_error.is_some() {
        crate::SyncStatus::Error
    } else {
        crate::SyncStatus::Idle
    };
    core.set_sync_state(sync_status, sync_error);
    core.set_sync_dirty(journal_dirty);
    Ok(())
}

pub(super) async fn execute(core: &mut KeelessCore, args: OpenArgs) -> Result<OperationSuccess> {
    run(core, args.target).await?;
    Ok(OperationSuccess::Open(EmptyResult {}))
}
