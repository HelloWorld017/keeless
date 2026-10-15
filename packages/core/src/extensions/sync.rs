use std::sync::{Arc, Mutex};

use keeless_kdbx::{CompositeCredentials, CompositeKey};
use keeless_schema::{OperationError, SyncStatus};
use keeless_sync::{RemoteFile, StorageError, SyncReport};

use crate::{CoreError, KeelessCore, Result, credential::CredentialVault};

type BackgroundFetch = Arc<Mutex<Option<std::result::Result<RemoteFile, StorageError>>>>;

pub(crate) struct SyncExtension {
    status: SyncStatus,
    error: Option<OperationError>,
    pending_key: Option<CompositeKey>,
    pending_rotation_key: Option<CompositeKey>,
    pending_rotation_credential: Option<CredentialVault>,
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
            pending_rotation_credential: None,
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
            let mut credential = self.sync_extension.pending_rotation_credential.take();
            self.sync_extension.status = SyncStatus::Syncing;
            self.sync_extension.error = None;
            if let Err(error) = self.persist_rotated_kdf_sync(&key, &mut credential).await {
                self.sync_extension.pending_rotation_key = Some(key);
                self.sync_extension.pending_rotation_credential = credential;
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
            Some(credentials) => {
                match self
                    .handle
                    .as_ref()
                    .ok_or(CoreError::DatabaseLocked)?
                    .derive_key(credentials)
                    .map_err(CoreError::from)
                {
                    Ok(key) => key,
                    // The remote may have changed its password as well as its KDF.
                    // Keep the local key separate from the remote credentials.
                    Err(CoreError::InvalidCredentials) if self.credential.is_some() => {
                        let key = self
                            .credential
                            .as_ref()
                            .expect("checked above")
                            .restore_key()?;
                        return self.sync_with_rotated_kdf(&key, credentials).await;
                    }
                    Err(error) => return Err(error),
                }
            }
            None => self
                .credential
                .as_ref()
                .ok_or(CoreError::DatabaseLocked)?
                .restore_key()?,
        };
        match self.sync_with_key(&key, None).await {
            Err(CoreError::CredentialsRequired) => {
                let credentials = match credentials {
                    Some(credentials) => credentials,
                    None if self.password_input.is_some() => {
                        let password = self
                            .request_password(crate::PasswordInputMode::Save)
                            .await?;
                        CompositeCredentials::new().with_password(&password)?
                    }
                    None => return Err(CoreError::CredentialsRequired),
                };
                self.sync_with_rotated_kdf(&key, &credentials).await
            }
            Ok(report) => {
                if !self.settings.paranoia_mode
                    && let Some(credentials) = &credentials
                {
                    if let Err(error) = self.prepare_next_if_missing(&key, credentials) {
                        self.set_sync_failure(&error);
                        return Err(error);
                    }
                }
                Ok(report)
            }
            Err(error) => Err(error),
        }
    }

    /// Explicitly rotate the database salt using a pre-derived key. Ordinary
    /// saves never consume the prepared slot. An empty slot requires credentials.
    pub async fn rotate_kdf(&mut self, password: Option<&[u8]>) -> Result<SyncReport> {
        self.enforce_auto_lock();
        self.sync_extension.clear_background();
        if self.handle.is_none() {
            return Err(CoreError::DatabaseLocked);
        }
        if self.sync_key_migration_pending() {
            return self.sync(password).await;
        }
        let prepared = self
            .credential
            .as_ref()
            .map(|vault| vault.restore_next())
            .transpose()?
            .flatten();
        let (old_key, parameters, next_key) = match (password, prepared) {
            (None, Some((parameters, next_key))) => (
                self.credential
                    .as_ref()
                    .expect("prepared slot has a vault")
                    .restore_key()?,
                parameters,
                next_key,
            ),
            (password, prepared) => {
                let password = match password {
                    Some(password) => zeroize::Zeroizing::new(password.to_vec()),
                    None => {
                        self.request_password(crate::PasswordInputMode::Save)
                            .await?
                    }
                };
                let credentials = CompositeCredentials::new().with_password(&password)?;
                let key = self
                    .handle
                    .as_ref()
                    .ok_or(CoreError::DatabaseLocked)?
                    .derive_key(&credentials)?;
                let (parameters, next_key) = match prepared {
                    Some(prepared) => prepared,
                    None => {
                        let vault = self.prepare_credential(&key, &credentials)?;
                        let prepared = vault.restore_next()?.ok_or(CoreError::Crypto)?;
                        if !self.settings.paranoia_mode {
                            // Retain the exact prepared pair across conflicts and failed writes.
                            self.credential = Some(vault);
                        }
                        prepared
                    }
                };
                (key, parameters, next_key)
            }
        };
        self.sync_extension.status = SyncStatus::Syncing;
        self.sync_extension.error = None;
        let result = self
            .handle
            .as_mut()
            .ok_or(CoreError::DatabaseLocked)?
            .rotate_kdf(&old_key, &next_key, &parameters)
            .await;
        let report = match result {
            Ok(report) => report,
            Err(error) => {
                let error = CoreError::from(error);
                self.set_sync_failure(&error);
                return Err(error);
            }
        };
        self.commit_rotated_sync(report, next_key, None).await
    }

    fn prepare_credential(
        &self,
        key: &CompositeKey,
        credentials: &CompositeCredentials,
    ) -> Result<CredentialVault> {
        CredentialVault::prepare(
            key,
            credentials,
            self.handle
                .as_ref()
                .ok_or(CoreError::DatabaseLocked)?
                .database()
                .kdf_parameters
                .as_ref()
                .ok_or(CoreError::Crypto)?,
        )
    }

    fn prepare_next_if_missing(
        &mut self,
        key: &CompositeKey,
        credentials: &CompositeCredentials,
    ) -> Result<()> {
        if self
            .credential
            .as_ref()
            .map(|vault| vault.restore_next())
            .transpose()?
            .flatten()
            .is_none()
        {
            self.credential = Some(self.prepare_credential(key, credentials)?);
        }
        Ok(())
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
        self.commit_rotated_sync(report, key, Some(credentials))
            .await
    }

    async fn commit_rotated_sync(
        &mut self,
        report: SyncReport,
        key: CompositeKey,
        credentials: Option<&CompositeCredentials>,
    ) -> Result<SyncReport> {
        let result = if self.settings.paranoia_mode {
            Ok(None)
        } else {
            match credentials {
                Some(credentials) => self.prepare_credential(&key, credentials),
                None => CredentialVault::wrap(&key),
            }
            .map(Some)
        };
        let mut credential = match result {
            Ok(credential) => credential,
            Err(error) => {
                // The remote already changed keys. Even a preparation failure
                // must leave enough state to complete local migration on retry.
                self.sync_extension.pending_rotation_key = Some(key);
                self.set_sync_failure(&error);
                return Err(error);
            }
        };
        if let Err(error) = self.persist_rotated_kdf_sync(&key, &mut credential).await {
            self.sync_extension.pending_rotation_key = Some(key);
            self.sync_extension.pending_rotation_credential = credential;
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

    async fn persist_rotated_kdf_sync(
        &mut self,
        key: &CompositeKey,
        credential: &mut Option<CredentialVault>,
    ) -> Result<()> {
        let database_id = self
            .selection
            .as_ref()
            .ok_or(CoreError::NoDatabaseSelected)?
            .database_id
            .clone();
        let journal =
            super::super::operations::mutations::MutationCoordinator::new(key, database_id, 0)?;
        if !self.settings.paranoia_mode && credential.is_none() {
            *credential = Some(CredentialVault::wrap(key)?);
        }
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
        self.credential = credential.take();
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
        self.sync_extension.pending_rotation_credential = None;
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
