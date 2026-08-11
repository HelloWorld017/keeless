//! Host-independent Keeless plaintext operation state machine.

mod config;
mod credential;
mod error;
mod extensions;
mod features;
mod host;
mod model;
mod network;
pub mod operations;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use config::{CONFIG_VERSION, PersistedConfig};
use credential::CredentialVault;
use keeless_kdbx::CompositeKey;
#[cfg(test)]
use keeless_kdbx::SecureArray;
use keeless_sync::{FileHandle, RemoteFile, StorageError, SyncReport};
use zeroize::Zeroizing;

pub use error::{CoreError, Result};
pub use host::{
    Clock, ConfigProvider, DatabasePersistence, HostFuture, KeelessHost, PasskeyConsentMode,
    PasskeyConsentProvider, PasskeyConsentRequest, PasswordInputMode, PasswordInputProvider,
    SystemClock, TaskSpawner, TransferProvider,
};
pub use keeless_schema;
pub use keeless_schema::{
    DatabaseStatus, KeelessConfig, KeelessConfigPatch, OperationError, StorageDescriptor,
    SyncStatus,
};
pub use keeless_sync::StorageProvider;
pub const MAX_REQUEST_SIZE: usize = 760 * 1024;
pub const MAX_REQUEST_ID_LENGTH: usize = 128;

/// Stable, non-secret identifier bound into database persistence authentication.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DatabaseId(Vec<u8>);

impl DatabaseId {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

struct Selection {
    descriptor: StorageDescriptor,
    provider: Arc<dyn StorageProvider>,
    database_id: DatabaseId,
    exists: bool,
}

type BackgroundFetch = Arc<Mutex<Option<std::result::Result<RemoteFile, StorageError>>>>;

pub struct KeelessCore {
    config_provider: Arc<dyn ConfigProvider>,
    password_input: Option<Arc<dyn PasswordInputProvider>>,
    passkey_consent: Option<Arc<dyn PasskeyConsentProvider>>,
    clock: Arc<dyn Clock>,
    storage_providers: HashMap<String, Arc<dyn StorageProvider>>,
    settings: KeelessConfig,
    selection: Option<Selection>,
    handle: Option<FileHandle>,
    credential: Option<CredentialVault>,
    extensions: extensions::Extensions,
    last_activity_ms: Option<u64>,
    persistence: Option<Arc<dyn DatabasePersistence>>,
    journal: Option<operations::mutations::MutationCoordinator>,
    sync_status: SyncStatus,
    sync_error: Option<OperationError>,
    pending_sync_key: Option<CompositeKey>,
    task_spawner: Option<Arc<dyn TaskSpawner>>,
    transfer_provider: Option<Arc<dyn TransferProvider>>,
    transfer_owner: Option<String>,
    background_fetch: Option<BackgroundFetch>,
    background_started_ms: Option<u64>,
    dirty: bool,
}

impl std::fmt::Debug for KeelessCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeelessCore")
            .field("settings", &self.settings)
            .field("has_selection", &self.selection.is_some())
            .field("unlocked", &self.handle.is_some())
            .field("credential", &self.credential)
            .finish_non_exhaustive()
    }
}

impl KeelessCore {
    pub async fn new(host: KeelessHost) -> Result<Self> {
        let loaded = host.config_provider.load().await?;
        let (settings, generated) = match loaded {
            Some(bytes) => {
                let bytes = Zeroizing::new(bytes);
                let persisted: PersistedConfig = serde_json::from_slice(&bytes)
                    .map_err(|error| CoreError::InvalidConfig(error.to_string()))?;
                persisted.validate()?;
                (persisted.settings.clone(), false)
            }
            None => (KeelessConfig::default(), true),
        };

        let core = Self {
            config_provider: host.config_provider,
            password_input: host.password_input,
            passkey_consent: host.passkey_consent,
            clock: host.clock,
            storage_providers: host.storage_providers,
            settings,
            selection: None,
            handle: None,
            credential: None,
            extensions: extensions::Extensions::new()?,
            last_activity_ms: None,
            persistence: host.database_persistence,
            journal: None,
            sync_status: SyncStatus::Idle,
            sync_error: None,
            pending_sync_key: None,
            task_spawner: host.task_spawner,
            transfer_provider: host.transfer_provider,
            transfer_owner: None,
            background_fetch: None,
            background_started_ms: None,
            dirty: false,
        };
        if generated {
            core.persist().await?;
        }
        Ok(core)
    }

    pub(crate) fn publish_download_transfer(&self, bytes: Zeroizing<Vec<u8>>) -> Result<String> {
        let provider = self
            .transfer_provider
            .as_ref()
            .ok_or_else(|| CoreError::Host("binary transfers are unavailable".into()))?;
        let owner = self
            .transfer_owner
            .as_deref()
            .ok_or_else(|| CoreError::Host("binary transfer owner is unavailable".into()))?;
        provider.publish_download(owner, bytes)
    }

    pub(crate) fn consume_upload_transfer(&self, transfer_id: &str) -> Result<Zeroizing<Vec<u8>>> {
        let provider = self
            .transfer_provider
            .as_ref()
            .ok_or_else(|| CoreError::Host("binary transfers are unavailable".into()))?;
        let owner = self
            .transfer_owner
            .as_deref()
            .ok_or_else(|| CoreError::Host("binary transfer owner is unavailable".into()))?;
        provider.consume_upload(owner, transfer_id)
    }

    pub(crate) fn clear_transfers(&self) {
        if let Some(provider) = &self.transfer_provider {
            provider.clear();
        }
    }

    pub(crate) async fn current_key(&self, password: Option<&[u8]>) -> Result<CompositeKey> {
        if self.handle.is_none() {
            return Err(CoreError::DatabaseLocked);
        }
        if let Some(password) = password {
            let key = CompositeKey::new().with_password(password)?;
            self.handle
                .as_ref()
                .ok_or(CoreError::DatabaseLocked)?
                .verify_credentials(&key)?;
            Ok(key)
        } else if let Some(credential) = &self.credential {
            credential.restore_key()
        } else {
            let password = self.request_password(PasswordInputMode::Save).await?;
            let key = CompositeKey::new().with_password(&password)?;
            self.handle
                .as_ref()
                .ok_or(CoreError::DatabaseLocked)?
                .verify_credentials(&key)?;
            Ok(key)
        }
    }

    pub async fn sync(&mut self, password: Option<&[u8]>) -> Result<SyncReport> {
        self.enforce_auto_lock();
        self.background_fetch = None;
        self.background_started_ms = None;
        self.pending_sync_key = None;
        let key = self.current_key(password).await?;
        self.sync_with_key(key, None).await
    }

    async fn sync_with_key(
        &mut self,
        key: CompositeKey,
        remote: Option<RemoteFile>,
    ) -> Result<SyncReport> {
        self.sync_status = SyncStatus::Syncing;
        self.sync_error = None;
        let handle = self.handle.as_mut().ok_or(CoreError::DatabaseLocked)?;
        let result = match remote {
            Some(remote) => handle.sync_from_remote(&key, remote).await,
            None => handle.sync(&key).await,
        };
        let report = match result {
            Ok(report) => report,
            Err(error) => {
                let error = CoreError::from(error);
                self.sync_status = SyncStatus::Error;
                self.sync_error = Some((&error).into());
                return Err(error);
            }
        };
        self.extensions.unlock(handle.database(), &key)?;
        if let Some(persistence) = &self.persistence {
            let database = self
                .handle
                .as_ref()
                .ok_or(CoreError::DatabaseLocked)?
                .checkpoint_bytes()
                .to_vec();
            let cache = self
                .journal
                .as_ref()
                .ok_or(CoreError::DatabaseLocked)?
                .encode_cache(&database)?;
            if let Err(error) = persistence.write_cache(&cache).await {
                self.sync_status = SyncStatus::Error;
                self.sync_error = Some((&error).into());
                return Err(error);
            }
        }
        if self
            .journal
            .as_ref()
            .is_some_and(|journal| journal.is_dirty())
            && let Some(persistence) = &self.persistence
        {
            if let Err(error) = persistence.clear_journal().await {
                self.sync_status = SyncStatus::Error;
                self.sync_error = Some((&error).into());
                return Err(error);
            }
            self.journal
                .as_mut()
                .expect("journal state checked")
                .mark_clean();
        }
        self.sync_status = SyncStatus::Idle;
        self.sync_error = None;
        self.dirty = false;
        self.last_activity_ms = Some(self.clock.monotonic_millis());
        Ok(report)
    }

    pub fn register_storage_provider(
        &mut self,
        name: impl Into<String>,
        provider: Arc<dyn StorageProvider>,
    ) {
        self.storage_providers.insert(name.into(), provider);
    }

    pub async fn tick(&mut self) {
        self.enforce_auto_lock();
        if self.handle.is_none() {
            self.pending_sync_key = None;
            self.background_fetch = None;
            self.background_started_ms = None;
            return;
        }

        if let Some(fetch) = &self.background_fetch {
            if self.background_started_ms.is_some_and(|started| {
                self.clock.monotonic_millis().saturating_sub(started) >= 30_000
            }) {
                self.background_fetch = None;
                self.background_started_ms = None;
                self.pending_sync_key = None;
                let error = CoreError::Host("background storage fetch timed out".into());
                self.sync_status = SyncStatus::Error;
                self.sync_error = Some((&error).into());
                return;
            }
            let completed = fetch.lock().ok().and_then(|mut result| result.take());
            if let Some(completed) = completed {
                self.background_fetch = None;
                self.background_started_ms = None;
                let Some(key) = self.pending_sync_key.take() else {
                    return;
                };
                match completed {
                    Ok(remote) => {
                        let _ = self.sync_with_key(key, Some(remote)).await;
                    }
                    Err(error) => {
                        let error = CoreError::from(error);
                        self.sync_status = SyncStatus::Error;
                        self.sync_error = Some((&error).into());
                    }
                }
            }
            return;
        }

        if let Some(key) = self.pending_sync_key.take() {
            let _ = self.sync_with_key(key, None).await;
            return;
        }

        if self.handle.as_ref().is_some_and(|handle| handle.is_dirty())
            && self.pending_sync_key.is_none()
            && let Some(credential) = &self.credential
            && let Ok(key) = credential.restore_key()
        {
            self.start_background_sync(key);
        }
    }

    fn start_background_sync(&mut self, key: CompositeKey) {
        let Some(spawner) = &self.task_spawner else {
            self.pending_sync_key = Some(key);
            return;
        };
        let Some(selection) = &self.selection else {
            return;
        };
        let provider = Arc::clone(&selection.provider);
        let path = selection.descriptor.path.clone();
        let result = Arc::new(Mutex::new(None));
        let task_result = Arc::clone(&result);
        spawner.spawn(Box::pin(async move {
            let fetched = provider.read(&path, None).await;
            if let Ok(mut result) = task_result.lock() {
                *result = Some(fetched);
            }
        }));
        self.pending_sync_key = Some(key);
        self.background_fetch = Some(result);
        self.background_started_ms = Some(self.clock.monotonic_millis());
        self.sync_status = SyncStatus::Syncing;
        self.sync_error = None;
    }

    fn enforce_auto_lock(&mut self) {
        let Some(timeout) = self.settings.auto_lock_timeout_ms else {
            return;
        };
        let Some(last_activity) = self.last_activity_ms else {
            return;
        };
        if self.clock.monotonic_millis().saturating_sub(last_activity) >= timeout {
            operations::lock::run(self);
        }
    }

    fn touch_activity(&mut self) {
        if self.handle.is_some() {
            self.last_activity_ms = Some(self.clock.monotonic_millis());
        }
    }

    async fn request_password(&self, mode: PasswordInputMode) -> Result<Zeroizing<Vec<u8>>> {
        let provider = self
            .password_input
            .as_ref()
            .ok_or(CoreError::PasswordRequired)?;
        provider
            .request_password(mode)
            .await?
            .ok_or(CoreError::PasswordRequired)
    }

    pub(crate) async fn request_passkey_consent(
        &self,
        request: PasskeyConsentRequest,
    ) -> Result<usize> {
        let provider = self
            .passkey_consent
            .as_ref()
            .ok_or(CoreError::PasskeyConsentRequired)?;
        provider
            .request_passkey_consent(request)
            .await?
            .ok_or(CoreError::PasskeyConsentDenied)
    }

    async fn persist(&self) -> Result<()> {
        let persisted = PersistedConfig {
            version: CONFIG_VERSION,
            settings: self.settings.clone(),
        };
        let bytes = zeroize::Zeroizing::new(serde_json::to_vec(&persisted)?);
        self.config_provider.save(&bytes).await
    }
}

fn random_array<const N: usize>() -> Result<[u8; N]> {
    let mut value = [0; N];
    getrandom::getrandom(&mut value).map_err(|_| CoreError::Crypto)?;
    Ok(value)
}

#[cfg(test)]
mod tests;
