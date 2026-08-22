use std::sync::{Arc, Mutex};

use keeless_kdbx::{CompositeCredentials, CompositeKey};
use keeless_schema::{OperationError, SyncStatus};
use keeless_sync::{RemoteFile, StorageError, SyncReport};

use crate::{CoreError, KeelessCore, Result};

type BackgroundFetch = Arc<Mutex<Option<std::result::Result<RemoteFile, StorageError>>>>;

pub(crate) struct SyncExtension {
    status: SyncStatus,
    error: Option<OperationError>,
    pending_key: Option<CompositeKey>,
    pending_rotation_key: Option<CompositeKey>,
    background_fetch: Option<BackgroundFetch>,
    background_started_ms: Option<u64>,
    dirty: bool,
}

impl SyncExtension {
    pub(crate) fn new() -> Self {
        Self {
            status: SyncStatus::Idle,
            error: None,
            pending_key: None,
            pending_rotation_key: None,
            background_fetch: None,
            background_started_ms: None,
            dirty: false,
        }
    }

    fn clear_background(&mut self) {
        self.pending_key = None;
        self.background_fetch = None;
        self.background_started_ms = None;
    }
}

impl KeelessCore {
    pub async fn sync(&mut self, password: Option<&[u8]>) -> Result<SyncReport> {
        self.enforce_auto_lock();
        self.sync_extension.clear_background();
        if self.handle.is_none() {
            return Err(CoreError::DatabaseLocked);
        }
        if let Some(key) = self.sync_extension.pending_rotation_key.take() {
            self.sync_extension.status = SyncStatus::Syncing;
            self.sync_extension.error = None;
            if let Err(error) = self.persist_rotated_kdf_sync(&key).await {
                self.sync_extension.pending_rotation_key = Some(key);
                self.set_sync_failure(&error);
                return Err(error);
            }
            return self.sync_with_key(&key, None).await;
        }
        let credentials = match password {
            Some(password) => Some(CompositeCredentials::new().with_password(password)?),
            None if self.credential.is_none() => {
                let password = self
                    .request_password(crate::PasswordInputMode::Save)
                    .await?;
                Some(CompositeCredentials::new().with_password(&password)?)
            }
            None => None,
        };
        let key = match &credentials {
            Some(credentials) => self
                .handle
                .as_ref()
                .ok_or(CoreError::DatabaseLocked)?
                .derive_key(credentials)
                .map_err(CoreError::from)?,
            None => self
                .credential
                .as_ref()
                .ok_or(CoreError::DatabaseLocked)?
                .restore_key()?,
        };
        match self.sync_with_key(&key, None).await {
            Err(CoreError::CredentialsRequired) if credentials.is_some() => {
                self.sync_with_rotated_kdf(&key, credentials.as_ref().expect("checked above"))
                    .await
            }
            result => result,
        }
    }

    pub(crate) async fn sync_with_key(
        &mut self,
        key: &CompositeKey,
        remote: Option<RemoteFile>,
    ) -> Result<SyncReport> {
        self.sync_extension.status = SyncStatus::Syncing;
        self.sync_extension.error = None;
        let result = {
            let handle = self.handle.as_mut().ok_or(CoreError::DatabaseLocked)?;
            match remote {
                Some(remote) => handle.sync_from_remote(key, remote).await,
                None => handle.sync(key).await,
            }
        };
        let report = match result {
            Ok(report) => report,
            Err(error) => {
                let error = CoreError::from(error);
                self.set_sync_failure(&error);
                return Err(error);
            }
        };
        if let Err(error) = self.persist_synced_database(key).await {
            self.set_sync_failure(&error);
            return Err(error);
        }
        self.sync_extension.status = SyncStatus::Idle;
        self.sync_extension.error = None;
        self.sync_extension.dirty = false;
        self.last_activity_ms = Some(self.clock.monotonic_millis());
        Ok(report)
    }

    async fn sync_with_rotated_kdf(
        &mut self,
        old_key: &CompositeKey,
        credentials: &CompositeCredentials,
    ) -> Result<SyncReport> {
        self.sync_extension.status = SyncStatus::Syncing;
        self.sync_extension.error = None;
        let result = {
            let handle = self.handle.as_mut().ok_or(CoreError::DatabaseLocked)?;
            handle.sync_kdf_rotated(old_key, credentials).await
        };
        let (report, key) = match result {
            Ok(result) => result,
            Err(error) => {
                let error = CoreError::from(error);
                self.set_sync_failure(&error);
                return Err(error);
            }
        };
        if let Err(error) = self.persist_rotated_kdf_sync(&key).await {
            self.sync_extension.pending_rotation_key = Some(key);
            self.set_sync_failure(&error);
            return Err(error);
        }
        self.sync_extension.status = SyncStatus::Idle;
        self.sync_extension.error = None;
        self.sync_extension.dirty = false;
        self.last_activity_ms = Some(self.clock.monotonic_millis());
        Ok(report)
    }

    async fn persist_synced_database(&mut self, key: &CompositeKey) -> Result<()> {
        let cache = {
            let handle = self.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
            self.extensions.unlock(handle.database(), key)?;
            self.journal
                .as_ref()
                .ok_or(CoreError::DatabaseLocked)?
                .encode_cache(
                    handle.checkpoint_bytes(),
                    handle
                        .database()
                        .kdf_parameters
                        .as_ref()
                        .ok_or(CoreError::Crypto)?,
                )?
        };
        self.persistence.write_cache(&cache).await?;
        if self
            .journal
            .as_ref()
            .is_some_and(|journal| journal.is_dirty())
        {
            self.persistence.clear_journal().await?;
            self.journal
                .as_mut()
                .expect("journal state checked")
                .mark_clean();
        }
        Ok(())
    }

    async fn persist_rotated_kdf_sync(&mut self, key: &CompositeKey) -> Result<()> {
        let database_id = self
            .selection
            .as_ref()
            .ok_or(CoreError::NoDatabaseSelected)?
            .database_id
            .clone();
        let journal =
            super::super::operations::mutations::MutationCoordinator::new(key, database_id, 0)?;
        let credential = if self.settings.paranoia_mode {
            None
        } else {
            Some(crate::credential::CredentialVault::wrap(key)?)
        };
        let cache = {
            let handle = self.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
            self.extensions.unlock(handle.database(), key)?;
            journal.encode_cache(
                handle.checkpoint_bytes(),
                handle
                    .database()
                    .kdf_parameters
                    .as_ref()
                    .ok_or(CoreError::Crypto)?,
            )?
        };
        self.persistence.write_cache(&cache).await?;
        self.persistence.clear_journal().await?;
        self.rotate_database_state(key).await?;
        self.journal = Some(journal);
        self.credential = credential;
        Ok(())
    }

    fn set_sync_failure(&mut self, error: &CoreError) {
        self.sync_extension.status = if matches!(error, CoreError::CredentialsRequired) {
            SyncStatus::CredentialsRequired
        } else {
            SyncStatus::Error
        };
        self.sync_extension.error = Some(error.into());
    }

    pub(crate) fn sync_key_migration_pending(&self) -> bool {
        self.sync_extension.pending_rotation_key.is_some()
    }

    pub(crate) async fn tick_sync(&mut self) {
        if self.handle.is_none() {
            self.sync_extension.clear_background();
            return;
        }
        if self.sync_extension.status == SyncStatus::CredentialsRequired {
            return;
        }
        if self.sync_extension.pending_rotation_key.is_some() {
            return;
        }

        if let Some(fetch) = &self.sync_extension.background_fetch {
            if self
                .sync_extension
                .background_started_ms
                .is_some_and(|started| {
                    self.clock.monotonic_millis().saturating_sub(started) >= 30_000
                })
            {
                self.sync_extension.clear_background();
                let error = CoreError::Host("background storage fetch timed out".into());
                self.sync_extension.status = SyncStatus::Error;
                self.sync_extension.error = Some((&error).into());
                return;
            }
            let completed = fetch.lock().ok().and_then(|mut result| result.take());
            if let Some(completed) = completed {
                self.sync_extension.background_fetch = None;
                self.sync_extension.background_started_ms = None;
                let Some(key) = self.sync_extension.pending_key.take() else {
                    return;
                };
                match completed {
                    Ok(remote) => {
                        let _ = self.sync_with_key(&key, Some(remote)).await;
                    }
                    Err(error) => {
                        let error = CoreError::from(error);
                        self.sync_extension.status = SyncStatus::Error;
                        self.sync_extension.error = Some((&error).into());
                    }
                }
            }
            return;
        }

        if let Some(key) = self.sync_extension.pending_key.take() {
            let _ = self.sync_with_key(&key, None).await;
            return;
        }

        if self.handle.as_ref().is_some_and(|handle| handle.is_dirty())
            && self.sync_extension.pending_key.is_none()
            && let Some(credential) = &self.credential
            && let Ok(key) = credential.restore_key()
        {
            self.start_background_sync(key);
        }
    }

    pub(crate) fn start_background_sync(&mut self, key: CompositeKey) {
        let Some(spawner) = &self.task_spawner else {
            self.sync_extension.pending_key = Some(key);
            return;
        };
        let Some(selection) = &self.selection else {
            return;
        };
        let Some(storage) = selection.storage.as_ref().cloned() else {
            return;
        };
        let Some(path) = selection
            .descriptor
            .as_ref()
            .map(|descriptor| descriptor.path.clone())
        else {
            return;
        };
        let result = Arc::new(Mutex::new(None));
        let task_result = Arc::clone(&result);
        spawner.spawn(Box::pin(async move {
            let fetched = storage.provider().read(&path, None).await;
            if let Ok(mut result) = task_result.lock() {
                *result = Some(fetched);
            }
        }));
        self.sync_extension.pending_key = Some(key);
        self.sync_extension.background_fetch = Some(result);
        self.sync_extension.background_started_ms = Some(self.clock.monotonic_millis());
        self.sync_extension.status = SyncStatus::Syncing;
        self.sync_extension.error = None;
    }

    pub(crate) fn reset_sync_state(&mut self) {
        self.sync_extension.clear_background();
        self.sync_extension.pending_rotation_key = None;
        if self.sync_extension.status == SyncStatus::Syncing {
            self.sync_extension.status = SyncStatus::Idle;
        }
    }

    pub(crate) fn set_sync_state(&mut self, status: SyncStatus, error: Option<OperationError>) {
        self.sync_extension.status = status;
        self.sync_extension.error = error;
    }

    pub(crate) fn set_sync_dirty(&mut self, dirty: bool) {
        self.sync_extension.dirty = dirty;
    }

    pub(crate) fn sync_status(&self) -> SyncStatus {
        self.sync_extension.status
    }

    pub(crate) fn sync_error(&self) -> Option<OperationError> {
        self.sync_extension.error.clone()
    }

    pub(crate) fn is_sync_dirty(&self) -> bool {
        self.sync_extension.dirty
    }
}
