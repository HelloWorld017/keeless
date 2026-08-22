use std::sync::Arc;

use crate::{
    CoreError, KeelessCore, Result, credential::CredentialVault,
    extensions::password_session::PasswordSessionExtension,
};
use keeless_kdbx::{CompositeCredentials, CompositeKey};
use keeless_schema::{EmptyResult, OperationSuccess, UnlockArgs};
use keeless_sync::{FileHandle, SyncError, SyncOptions};

pub(crate) async fn run(core: &mut KeelessCore, password: &[u8]) -> Result<()> {
    let database_id = core
        .selection
        .as_ref()
        .map(|selection| selection.database_id.clone())
        .ok_or(CoreError::NoDatabaseSelected)?;
    let credentials = CompositeCredentials::new().with_password(password)?;
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
    let cached = persistence.read_cache().await?;
    let (mut handle, mut key, mut journal, mut opened_from_cache, mut recovered_error) =
        if let Some(cache) = cached {
            match super::mutations::MutationCoordinator::cache_kdf_parameters(&cache)
                .and_then(|parameters| credentials.derive_key(&parameters).map_err(Into::into))
                .and_then(|key| {
                    let mut journal =
                        super::mutations::MutationCoordinator::new(&key, database_id.clone(), 0)?;
                    let (sequence, bytes) = journal.decode_cache(&cache)?;
                    let handle = FileHandle::open_cached(
                        Arc::clone(&provider),
                        path.clone(),
                        bytes,
                        &key,
                        SyncOptions::default(),
                    )?;
                    journal.set_sequence(sequence);
                    Ok::<_, CoreError>((handle, key, journal))
                }) {
                Ok((handle, key, journal)) => (handle, key, journal, true, None),
                Err(cache_error) => {
                    persistence
                        .quarantine_cache(&cache_error.to_string())
                        .await?;
                    let (handle, key) = open_remote_selected(
                        core,
                        Arc::clone(&provider),
                        path.clone(),
                        &credentials,
                    )
                    .await?;
                    let journal =
                        super::mutations::MutationCoordinator::new(&key, database_id.clone(), 0)?;
                    (handle, key, journal, false, Some(cache_error))
                }
            }
        } else {
            let (handle, key) =
                open_remote_selected(core, Arc::clone(&provider), path.clone(), &credentials)
                    .await?;
            let journal = super::mutations::MutationCoordinator::new(&key, database_id.clone(), 0)?;
            (handle, key, journal, false, None)
        };
    match persistence.read_journal().await {
        Ok(lines) => {
            if let Err(error) =
                super::mutations::replay_lines(&mut journal, handle.replay_database(), &key, &lines)
            {
                let (remote, remote_key) =
                    open_remote_selected(core, Arc::clone(&provider), path.clone(), &credentials)
                        .await?;
                persistence.quarantine_journal(&error.to_string()).await?;
                handle = remote;
                key = remote_key;
                opened_from_cache = false;
                recovered_error = Some(error);
                journal = super::mutations::MutationCoordinator::new(&key, database_id.clone(), 0)?;
            }
        }
        Err(error) => {
            let (remote, remote_key) =
                open_remote_selected(core, Arc::clone(&provider), path.clone(), &credentials)
                    .await?;
            persistence.quarantine_journal(&error.to_string()).await?;
            handle = remote;
            key = remote_key;
            opened_from_cache = false;
            recovered_error = Some(error);
            journal = super::mutations::MutationCoordinator::new(&key, database_id.clone(), 0)?;
        }
    }
    if journal.is_dirty() {
        handle.mark_dirty();
    }
    let should_sync = opened_from_cache || journal.is_dirty();
    if !opened_from_cache && !journal.is_dirty() {
        let cache = journal.encode_cache(
            handle.checkpoint_bytes(),
            handle
                .database()
                .kdf_parameters
                .as_ref()
                .ok_or(CoreError::Crypto)?,
        )?;
        persistence.write_cache(&cache).await?;
    }
    core.activate_database_state(&key).await?;
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
        Some(CredentialVault::wrap(&key)?)
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
    credentials: &CompositeCredentials,
) -> Result<(FileHandle, CompositeKey)> {
    match FileHandle::open(provider, path, credentials, SyncOptions::default()).await {
        Ok(handle) => Ok(handle),
        Err(SyncError::RemoteNotFound) => Err(CoreError::DatabaseNotFound),
        Err(error) => Err(error.into()),
    }
}

async fn open_remote_selected(
    core: &mut KeelessCore,
    provider: Arc<dyn crate::StorageProvider>,
    path: String,
    key: impl Into<OpenRemoteKey<'_>>,
) -> Result<(FileHandle, CompositeKey)> {
    let result = match key.into() {
        OpenRemoteKey::Credentials(credentials) => open_remote(provider, path, credentials).await,
        OpenRemoteKey::Derived(key) => {
            let handle = FileHandle::open_with_key(provider, path, key, SyncOptions::default())
                .await
                .map_err(CoreError::from)?;
            Ok((handle, key.try_clone()?))
        }
    };
    if matches!(result, Err(CoreError::DatabaseNotFound)) && core.handle.is_none() {
        if let Some(selection) = &mut core.selection {
            selection.exists = false;
        }
    }
    result
}

enum OpenRemoteKey<'a> {
    Credentials(&'a CompositeCredentials),
    Derived(&'a CompositeKey),
}

impl<'a> From<&'a CompositeCredentials> for OpenRemoteKey<'a> {
    fn from(value: &'a CompositeCredentials) -> Self {
        Self::Credentials(value)
    }
}

impl<'a> From<&'a CompositeKey> for OpenRemoteKey<'a> {
    fn from(value: &'a CompositeKey) -> Self {
        Self::Derived(value)
    }
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
