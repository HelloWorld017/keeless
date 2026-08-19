use std::sync::Arc;

use crate::{
    CoreError, KeelessCore, Result, credential::CredentialVault,
    extensions::password_session::PasswordSessionExtension,
};
use keeless_kdbx::CompositeKey;
use keeless_schema::{EmptyResult, OperationSuccess, UnlockArgs};
use keeless_sync::{FileHandle, SyncError, SyncOptions};

pub(crate) async fn run(core: &mut KeelessCore, password: &[u8]) -> Result<()> {
    let database_id = core
        .selection
        .as_ref()
        .map(|selection| selection.database_id.clone())
        .ok_or(CoreError::NoDatabaseSelected)?;
    let key = CompositeKey::new().with_password(password)?;
    let raw_key = key.build_raw_key()?;
    core.restore_recent_selection(&raw_key).await?;
    let (provider, path) = core
        .selection
        .as_ref()
        .map(|selection| {
            (
                selection.storage.as_ref().map(|storage| storage.provider()),
                selection
                    .descriptor
                    .as_ref()
                    .map(|descriptor| descriptor.path.clone()),
            )
        })
        .ok_or(CoreError::NoDatabaseSelected)?;
    let provider = provider.ok_or(CoreError::RecentDatabaseUnavailable)?;
    let path = path.ok_or(CoreError::RecentDatabaseUnavailable)?;
    let persistence = core.persistence.clone();
    let mut journal = super::mutations::MutationCoordinator::new(&raw_key, database_id.clone(), 0)?;
    let cached = persistence.read_cache().await?;
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
                persistence
                    .quarantine_cache(&cache_error.to_string())
                    .await?;
                recovered_error = Some(cache_error);
                remote
            }
        }
    } else {
        open_remote_selected(core, Arc::clone(&provider), path.clone(), &key).await?
    };
    match persistence.read_journal().await {
        Ok(lines) => {
            if let Err(error) =
                super::mutations::replay_lines(&mut journal, handle.replay_database(), &key, &lines)
            {
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
        Err(error) => {
            let remote =
                open_remote_selected(core, Arc::clone(&provider), path.clone(), &key).await?;
            persistence.quarantine_journal(&error.to_string()).await?;
            handle = remote;
            opened_from_cache = false;
            recovered_error = Some(error);
            journal = super::mutations::MutationCoordinator::new(&raw_key, database_id.clone(), 0)?;
        }
    }
    if journal.is_dirty() {
        handle.mark_dirty();
    }
    let should_sync = opened_from_cache || journal.is_dirty();
    if !opened_from_cache && !journal.is_dirty() {
        let cache = journal.encode_cache(handle.checkpoint_bytes())?;
        persistence.write_cache(&cache).await?;
    }
    core.activate_database_state(&raw_key).await?;
    if core.selection.as_ref().is_some_and(|selection| {
        selection
            .storage
            .as_ref()
            .is_some_and(|storage| storage.is_persistent())
    }) {
        core.persist().await?;
    }
    let credential = if core.settings.paranoia_mode {
        None
    } else {
        Some(CredentialVault::wrap(&raw_key)?)
    };
    core.extensions.unlock(handle.database(), &key)?;
    core.credential = credential;
    core.handle = Some(handle);
    core.journal = Some(journal);
    if should_sync {
        core.start_background_sync(key);
    } else {
        core.reset_sync_state();
    }
    let status = if should_sync {
        crate::SyncStatus::Syncing
    } else if recovered_error.is_some() {
        crate::SyncStatus::Error
    } else {
        crate::SyncStatus::Idle
    };
    core.set_sync_state(status, recovered_error.as_ref().map(Into::into));
    core.set_sync_dirty(
        core.journal
            .as_ref()
            .is_some_and(|journal| journal.is_dirty()),
    );
    if let Some(selection) = &mut core.selection {
        selection.exists = true;
    }
    core.last_activity_ms = Some(core.clock.monotonic_millis());
    crate::recent::record_success(core).await?;
    Ok(())
}

async fn open_remote(
    provider: Arc<dyn crate::StorageProvider>,
    path: String,
    key: &CompositeKey,
) -> Result<FileHandle> {
    match FileHandle::open(provider, path, key, SyncOptions::default()).await {
        Ok(handle) => Ok(handle),
        Err(SyncError::RemoteNotFound) => Err(CoreError::DatabaseNotFound),
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
    let password_argument = {
        let now_millis = core.clock.monotonic_millis();
        core.extensions
            .get_mut::<PasswordSessionExtension>()
            .resolve_argument(
                args.password.take(),
                args.password_session.take(),
                now_millis,
            )?
    };
    let password = match password_argument {
        Some(password) => password,
        None => {
            core.request_password(crate::PasswordInputMode::Unlock)
                .await?
        }
    };
    run(core, &password).await?;
    Ok(OperationSuccess::Unlock(EmptyResult {}))
}
