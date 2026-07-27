use std::sync::Arc;

use keeless_kdbx::CompositeKey;
use keeless_schema::{EmptyResult, OperationSuccess, UnlockArgs};
use keeless_sync::{FileHandle, SyncError, SyncOptions};
use zeroize::Zeroizing;

use crate::{CoreError, KeelessCore, Result, credential::CredentialVault};

pub(crate) async fn run(core: &mut KeelessCore, password: &[u8]) -> Result<()> {
    let (provider, path, database_id) = core
        .selection
        .as_ref()
        .map(|selection| {
            (
                Arc::clone(&selection.provider),
                selection.descriptor.path.clone(),
                selection.database_id.clone(),
            )
        })
        .ok_or(CoreError::NoDatabaseSelected)?;
    let key = CompositeKey::new().with_password(password)?;
    let raw_key = key.build_raw_key()?;
    let persistence = core.persistence.clone();
    let mut journal = super::mutations::MutationCoordinator::new(&raw_key, database_id.clone(), 0)?;
    let cached = match &persistence {
        Some(persistence) => persistence.read_cache().await?,
        None => None,
    };
    let mut opened_from_cache = false;
    let mut recovered_error = None;
    let mut handle = if let Some(cache) = cached {
        match journal.decode_cache(&cache).and_then(|(sequence, bytes)| {
            FileHandle::open_cached(
                Arc::clone(&provider),
                path.clone(),
                bytes,
                &key,
                SyncOptions::default(),
            )
            .map_err(CoreError::from)
            .map(|handle| (sequence, handle))
        }) {
            Ok((sequence, handle)) => {
                journal.set_sequence(sequence);
                opened_from_cache = true;
                handle
            }
            Err(cache_error) => {
                let remote =
                    open_remote_selected(core, Arc::clone(&provider), path.clone(), &key).await?;
                if let Some(persistence) = &persistence {
                    persistence
                        .quarantine_cache(&cache_error.to_string())
                        .await?;
                }
                recovered_error = Some(cache_error);
                remote
            }
        }
    } else {
        open_remote_selected(core, Arc::clone(&provider), path.clone(), &key).await?
    };
    if let Some(persistence) = &persistence {
        match persistence.read_journal().await {
            Ok(lines) => {
                if let Err(error) = super::mutations::replay_lines(
                    &mut journal,
                    handle.replay_database(),
                    &key,
                    &lines,
                ) {
                    let remote =
                        open_remote_selected(core, Arc::clone(&provider), path.clone(), &key)
                            .await?;
                    persistence.quarantine_journal(&error.to_string()).await?;
                    handle = remote;
                    opened_from_cache = false;
                    recovered_error = Some(error);
                    journal = super::mutations::MutationCoordinator::new(
                        &raw_key,
                        database_id.clone(),
                        0,
                    )?;
                }
            }
            Err(error) => {
                let remote =
                    open_remote_selected(core, Arc::clone(&provider), path.clone(), &key).await?;
                persistence.quarantine_journal(&error.to_string()).await?;
                handle = remote;
                opened_from_cache = false;
                recovered_error = Some(error);
                journal =
                    super::mutations::MutationCoordinator::new(&raw_key, database_id.clone(), 0)?;
            }
        }
    }
    if journal.is_dirty() {
        handle.mark_dirty();
    }
    let should_sync = opened_from_cache || journal.is_dirty();
    if !opened_from_cache && !journal.is_dirty() {
        if let Some(persistence) = &persistence {
            let cache = journal.encode_cache(handle.checkpoint_bytes())?;
            persistence.write_cache(&cache).await?;
        }
    }
    let credential = if core.settings.paranoia_mode {
        None
    } else {
        Some(CredentialVault::wrap(&raw_key)?)
    };
    core.extensions.unlock(handle.database(), &key)?;
    core.handle = None;
    core.credential = credential;
    core.handle = Some(handle);
    core.journal = Some(journal);
    core.dirty = core
        .journal
        .as_ref()
        .is_some_and(|journal| journal.is_dirty());
    if should_sync {
        core.start_background_sync(key);
    } else {
        core.pending_sync_key = None;
    }
    core.sync_status = if should_sync {
        crate::SyncStatus::Syncing
    } else if recovered_error.is_some() {
        crate::SyncStatus::Error
    } else {
        crate::SyncStatus::Idle
    };
    core.sync_error = recovered_error.as_ref().map(Into::into);
    if let Some(selection) = &mut core.selection {
        selection.exists = true;
    }
    core.last_activity_ms = Some(core.clock.monotonic_millis());
    Ok(())
}

async fn open_remote(
    provider: Arc<dyn crate::StorageProvider>,
    path: String,
    key: &CompositeKey,
) -> Result<FileHandle> {
    match FileHandle::open(provider, path, key, SyncOptions::default()).await {
        Ok(handle) => Ok(handle),
        Err(SyncError::RemoteNotFound(_)) => Err(CoreError::DatabaseNotFound),
        Err(error) => Err(error.into()),
    }
}

async fn open_remote_selected(
    core: &mut KeelessCore,
    provider: Arc<dyn crate::StorageProvider>,
    path: String,
    key: &CompositeKey,
) -> Result<FileHandle> {
    let result = open_remote(provider, path, key).await;
    if matches!(result, Err(CoreError::DatabaseNotFound)) && core.handle.is_none() {
        if let Some(selection) = &mut core.selection {
            selection.exists = false;
        }
    }
    result
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: UnlockArgs,
) -> Result<OperationSuccess> {
    let password = match args.password.take() {
        Some(password) => Zeroizing::new(password.into_bytes()),
        None => {
            core.request_password(crate::PasswordInputMode::Unlock)
                .await?
        }
    };
    run(core, &password).await?;
    Ok(OperationSuccess::Unlock(EmptyResult {}))
}
