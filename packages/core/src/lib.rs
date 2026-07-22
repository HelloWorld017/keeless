//! Host-independent Keeless plaintext operation state machine.

mod config;
mod credential;
mod error;
mod host;
mod model;
mod network;
mod operations;
mod tag_styles;

use std::{collections::HashMap, sync::Arc};

use config::{CONFIG_VERSION, PersistedConfig};
use credential::CredentialVault;
use keeless_kdbx::CompositeKey;
#[cfg(test)]
use keeless_kdbx::SecureArray;
use keeless_sync::{FileHandle, SyncReport};
use zeroize::Zeroizing;

pub use error::{CoreError, Result};
pub use host::{
    Clock, ConfigProvider, HostFuture, KeelessHost, PasswordInputMode, PasswordInputProvider,
    SystemClock,
};
pub use keeless_schema;
pub use keeless_schema::{DatabaseStatus, KeelessConfig, KeelessConfigPatch, StorageDescriptor};
pub use keeless_sync::StorageProvider;
pub const MAX_REQUEST_SIZE: usize = 256 * 1024;
pub const MAX_REQUEST_ID_LENGTH: usize = 128;

struct Selection {
    descriptor: StorageDescriptor,
    provider: Arc<dyn StorageProvider>,
    exists: bool,
}

pub struct KeelessCore {
    config_provider: Arc<dyn ConfigProvider>,
    password_input: Option<Arc<dyn PasswordInputProvider>>,
    clock: Arc<dyn Clock>,
    storage_providers: HashMap<String, Arc<dyn StorageProvider>>,
    settings: KeelessConfig,
    selection: Option<Selection>,
    handle: Option<FileHandle>,
    credential: Option<CredentialVault>,
    last_activity_ms: Option<u64>,
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
            clock: host.clock,
            storage_providers: host.storage_providers,
            settings,
            selection: None,
            handle: None,
            credential: None,
            last_activity_ms: None,
        };
        if generated {
            core.persist().await?;
        }
        Ok(core)
    }

    pub async fn sync(&mut self, password: Option<&[u8]>) -> Result<SyncReport> {
        self.enforce_auto_lock();
        if self.handle.is_none() {
            return Err(CoreError::DatabaseLocked);
        }
        let key = if let Some(password) = password {
            let key = CompositeKey::new().with_password(password)?;
            self.handle
                .as_ref()
                .ok_or(CoreError::DatabaseLocked)?
                .verify_credentials(&key)?;
            key
        } else if let Some(credential) = &self.credential {
            credential.restore_key()?
        } else {
            let password = self.request_password(PasswordInputMode::Save).await?;
            let key = CompositeKey::new().with_password(&password)?;
            self.handle
                .as_ref()
                .ok_or(CoreError::DatabaseLocked)?
                .verify_credentials(&key)?;
            key
        };
        let report = self
            .handle
            .as_mut()
            .ok_or(CoreError::DatabaseLocked)?
            .sync(&key)
            .await?;
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

    pub fn tick(&mut self) {
        self.enforce_auto_lock();
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
