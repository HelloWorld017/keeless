//! Host-independent Keeless plaintext operation state machine.

mod config;
mod credential;
mod database_state;
mod error;
mod extensions;
mod features;
mod host;
mod model;
mod network;
pub mod operations;
mod recent;

use std::{collections::HashMap, sync::Arc};

use config::{CONFIG_VERSION, PersistedConfig};
use credential::CredentialVault;
use database_state::{CONFIG_RECORD, CORE_WIRE_RECORD, EncryptedDatabaseStateStore};
use keeless_kdbx::{CompositeCredentials, CompositeKey};
use keeless_sync::FileHandle;
use zeroize::Zeroizing;

pub use error::{CoreError, Result};
pub use host::{
    Clock, ConnectionApprovalKind, ConnectionApprovalProvider, ConnectionApprovalRequest,
    DatabasePersistence, HostFuture, KeelessHost, PasskeyConsentMode, PasskeyConsentProvider,
    PasskeyConsentRequest, PasswordInputMode, PasswordInputProvider, Storage, SystemClock,
    TaskSpawner, TransferProvider,
};
pub use keeless_schema;
pub use keeless_schema::{
    DatabaseStatus, KeelessConfig, KeelessConfigPatch, OperationError, StorageDescriptor,
    SyncStatus,
};
pub use keeless_sync::StorageProvider;
pub const MAX_REQUEST_SIZE: usize = 760 * 1024;
pub const MAX_REQUEST_ID_LENGTH: usize = 128;
const DATABASE_ID_HKDF_INFO: &[u8] = b"keeless database id v1";

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

    pub(crate) fn from_storage(provider: &str, path: &str) -> Result<Self> {
        use hkdf::Hkdf;
        use sha2::Sha256;

        let mut input = Vec::with_capacity(provider.len() + path.len() + 16);
        input.extend_from_slice(&(provider.len() as u64).to_be_bytes());
        input.extend_from_slice(provider.as_bytes());
        input.extend_from_slice(&(path.len() as u64).to_be_bytes());
        input.extend_from_slice(path.as_bytes());
        let mut id = [0; 32];
        Hkdf::<Sha256>::new(None, &input)
            .expand(DATABASE_ID_HKDF_INFO, &mut id)
            .map_err(|_| CoreError::Crypto)?;
        Ok(Self(id.to_vec()))
    }

    pub(crate) fn from_recent_id(id: &str) -> Result<Self> {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

        URL_SAFE_NO_PAD
            .decode(id)
            .map(Self)
            .map_err(|_| CoreError::InvalidRecentDatabase)
    }

    pub(crate) fn recent_id(&self) -> String {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

        URL_SAFE_NO_PAD.encode(&self.0)
    }

    pub fn encoded(&self) -> String {
        self.recent_id()
    }
}

struct Selection {
    descriptor: Option<StorageDescriptor>,
    storage: Option<Arc<Storage>>,
    database_id: DatabaseId,
    exists: bool,
}

pub struct KeelessCore {
    network: network::Network,
    encrypted_state: Option<EncryptedDatabaseStateStore>,
    password_input: Option<Arc<dyn PasswordInputProvider>>,
    passkey_consent: Option<Arc<dyn PasskeyConsentProvider>>,
    clock: Arc<dyn Clock>,
    storage_providers: HashMap<String, Arc<Storage>>,
    settings: KeelessConfig,
    selection: Option<Selection>,
    handle: Option<FileHandle>,
    credential: Option<CredentialVault>,
    extensions: extensions::Extensions,
    sync_extension: extensions::sync::SyncExtension,
    last_activity_ms: Option<u64>,
    persistence: Arc<dyn DatabasePersistence>,
    core_state: Arc<dyn keeless_lesswire::StateStore>,
    journal: Option<operations::mutations::MutationCoordinator>,
    task_spawner: Option<Arc<dyn TaskSpawner>>,
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
        let network = network::Network::new(&host).await?;

        Ok(Self {
            network,
            encrypted_state: None,
            password_input: host.password_input,
            passkey_consent: host.passkey_consent,
            clock: host.clock,
            storage_providers: host.storage_providers,
            settings: KeelessConfig::default(),
            selection: None,
            handle: None,
            credential: None,
            extensions: extensions::Extensions::new()?,
            sync_extension: extensions::sync::SyncExtension::new(),
            last_activity_ms: None,
            persistence: host.database_persistence,
            core_state: host.core_state,
            journal: None,
            task_spawner: host.task_spawner,
        })
    }

    pub(crate) async fn current_key(&self, password: Option<&[u8]>) -> Result<CompositeKey> {
        if self.handle.is_none() {
            return Err(CoreError::DatabaseLocked);
        }
        if let Some(password) = password {
            let credentials = CompositeCredentials::new().with_password(password)?;
            Ok(self
                .handle
                .as_ref()
                .ok_or(CoreError::DatabaseLocked)?
                .derive_key(&credentials)?)
        } else if let Some(credential) = &self.credential {
            credential.restore_key()
        } else {
            let password = self.request_password(PasswordInputMode::Save).await?;
            let credentials = CompositeCredentials::new().with_password(&password)?;
            Ok(self
                .handle
                .as_ref()
                .ok_or(CoreError::DatabaseLocked)?
                .derive_key(&credentials)?)
        }
    }

    pub fn register_storage_provider(&mut self, name: impl Into<String>, storage: Arc<Storage>) {
        self.storage_providers.insert(name.into(), storage);
    }

    pub async fn tick(&mut self) {
        self.extensions.tick(self.clock.monotonic_millis());
        self.enforce_auto_lock();
        self.tick_sync().await;
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
        let state = self
            .encrypted_state
            .as_ref()
            .ok_or(CoreError::DatabaseLocked)?;
        let persisted = PersistedConfig {
            version: CONFIG_VERSION,
            settings: self.settings.clone(),
            storage: self.selection.as_ref().and_then(|selection| {
                selection
                    .storage
                    .as_ref()
                    .filter(|storage| storage.is_persistent())
                    .and_then(|_| selection.descriptor.clone())
            }),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&persisted)?);
        state.save_record(CONFIG_RECORD, &bytes).await
    }

    pub(crate) async fn activate_database_state(&mut self, key: &CompositeKey) -> Result<()> {
        let persistence = self.persistence.clone();
        let database_id = self
            .selection
            .as_ref()
            .ok_or(CoreError::NoDatabaseSelected)?
            .database_id
            .clone();
        let state = EncryptedDatabaseStateStore::new(key, persistence, database_id)?;
        let persisted = match state.load_record(CONFIG_RECORD).await? {
            Some(bytes) => {
                let persisted: PersistedConfig = serde_json::from_slice(&bytes)
                    .map_err(|error| CoreError::InvalidConfig(error.to_string()))?;
                persisted.validate()?;
                persisted
            }
            None => {
                let persisted = PersistedConfig {
                    version: CONFIG_VERSION,
                    settings: KeelessConfig::default(),
                    storage: None,
                };
                state
                    .save_record(
                        CONFIG_RECORD,
                        &Zeroizing::new(serde_json::to_vec(&persisted)?),
                    )
                    .await?;
                persisted
            }
        };
        self.settings = persisted.settings;
        self.network.activate_core_server(&state).await?;
        self.encrypted_state = Some(state);
        Ok(())
    }

    pub(crate) async fn rotate_database_state(&mut self, key: &CompositeKey) -> Result<()> {
        let previous = self
            .encrypted_state
            .as_ref()
            .ok_or(CoreError::DatabaseLocked)?
            .clone();
        let config = previous.load_record(CONFIG_RECORD).await?;
        let core_wire = previous.load_record(CORE_WIRE_RECORD).await?;
        let database_id = self
            .selection
            .as_ref()
            .ok_or(CoreError::NoDatabaseSelected)?
            .database_id
            .clone();
        let state = EncryptedDatabaseStateStore::new(key, self.persistence.clone(), database_id)?;
        if let Some(config) = config {
            state.save_record(CONFIG_RECORD, &config).await?;
        }
        if let Some(core_wire) = core_wire {
            state.save_record(CORE_WIRE_RECORD, &core_wire).await?;
        }
        self.network.activate_core_server(&state).await?;
        self.encrypted_state = Some(state);
        Ok(())
    }
}

fn random_array<const N: usize>() -> Result<[u8; N]> {
    let mut value = [0; N];
    getrandom::getrandom(&mut value).map_err(|_| CoreError::Crypto)?;
    Ok(value)
}

#[cfg(test)]
mod tests;
