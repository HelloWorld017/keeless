//! Host-independent Keeless state machine and authenticated frame protocol.

mod auth;
mod config;
mod credential;
mod error;
mod host;
mod model;
mod network;
mod operations;
mod protocol;

use std::{collections::HashMap, sync::Arc};

use auth::Identity;
use config::{CONFIG_VERSION, PersistedConfig, PersistedIdentity, decode_32, encode};
use credential::CredentialVault;
use keeless_kdbx::{CompositeKey, SecureArray};
use keeless_sync::{FileHandle, SyncReport};
use protocol::parse_public_key_bundle;
use zeroize::{Zeroize, Zeroizing};

pub use error::{CoreError, Result};
pub use host::{
    ClientApprovalProvider, Clock, ConfigProvider, HostFuture, KeelessHost, SystemClock,
};
pub use keeless_schema;
pub use keeless_schema::{
    DatabaseStatus, KeelessConfig, KeelessConfigPatch, MessageFrame, StorageDescriptor,
};
pub use keeless_sync::StorageProvider;
pub use protocol::{
    FRAME_TIMESTAMP_TOLERANCE_MS, FRAME_TRANSCRIPT_PREFIX, FRAME_VERSION, MAX_FRAME_SIZE,
    MAX_REQUEST_ID_LENGTH, MAX_REQUEST_SIZE, NONCE_CACHE_CAPACITY, PAYLOAD_HEADER_PREFIX,
    PAYLOAD_HKDF_INFO,
};

struct Selection {
    descriptor: StorageDescriptor,
    provider: Arc<dyn StorageProvider>,
    exists: bool,
}

pub struct KeelessCore {
    config_provider: Arc<dyn ConfigProvider>,
    approval_provider: Arc<dyn ClientApprovalProvider>,
    clock: Arc<dyn Clock>,
    storage_providers: HashMap<String, Arc<dyn StorageProvider>>,
    settings: KeelessConfig,
    identity: Identity,
    approved_clients: Vec<String>,
    selection: Option<Selection>,
    handle: Option<FileHandle>,
    credential: Option<CredentialVault>,
    last_activity_ms: Option<u64>,
    nonce_cache: HashMap<String, u64>,
}

impl std::fmt::Debug for KeelessCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeelessCore")
            .field("settings", &self.settings)
            .field("identity", &self.identity)
            .field("approved_client_count", &self.approved_clients.len())
            .field("has_selection", &self.selection.is_some())
            .field("unlocked", &self.handle.is_some())
            .field("credential", &self.credential)
            .finish_non_exhaustive()
    }
}

impl KeelessCore {
    pub async fn new(host: KeelessHost) -> Result<Self> {
        let loaded = host.config_provider.load().await?;
        let (settings, identity, mut approved_clients, generated) = match loaded {
            Some(bytes) => {
                let bytes = Zeroizing::new(bytes);
                let persisted: PersistedConfig = serde_json::from_slice(&bytes)
                    .map_err(|error| CoreError::InvalidConfig(error.to_string()))?;
                persisted.validate()?;
                let mut signing = decode_32(&persisted.identity.ed25519_signing_seed)?;
                let mut encryption = decode_32(&persisted.identity.x25519_static_secret)?;
                (
                    persisted.settings.clone(),
                    Identity {
                        signing_seed: SecureArray::from_array_mut(&mut signing)?,
                        x25519_secret: SecureArray::from_array_mut(&mut encryption)?,
                    },
                    persisted.approved_client_bundles.clone(),
                    false,
                )
            }
            None => {
                let mut signing = random_array::<32>()?;
                let mut encryption = random_array::<32>()?;
                (
                    KeelessConfig::default(),
                    Identity {
                        signing_seed: SecureArray::from_array_mut(&mut signing)?,
                        x25519_secret: SecureArray::from_array_mut(&mut encryption)?,
                    },
                    Vec::new(),
                    true,
                )
            }
        };

        let mut changed = generated;
        for bundle in host.default_approved_keys {
            parse_public_key_bundle(&bundle).ok_or_else(|| {
                CoreError::InvalidConfig("invalid default approved client bundle".into())
            })?;
            if !approved_clients.contains(&bundle) {
                approved_clients.push(bundle);
                changed = true;
            }
        }

        let core = Self {
            config_provider: host.config_provider,
            approval_provider: host.approval_provider,
            clock: host.clock,
            storage_providers: host.storage_providers,
            settings,
            identity,
            approved_clients,
            selection: None,
            handle: None,
            credential: None,
            last_activity_ms: None,
            nonce_cache: HashMap::new(),
        };
        if changed {
            core.persist().await?;
        }
        Ok(core)
    }

    pub async fn sync(&mut self, password: Option<&[u8]>) -> Result<SyncReport> {
        self.enforce_auto_lock();
        let handle = self.handle.as_mut().ok_or(CoreError::DatabaseLocked)?;
        let report = if let Some(password) = password {
            let key = CompositeKey::new().with_password(password)?;
            handle.verify_credentials(&key)?;
            handle.sync(&key).await?
        } else {
            let credential = self
                .credential
                .as_ref()
                .ok_or(CoreError::PasswordRequired)?;
            credential.sync(handle).await?
        };
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

    // FIXME ai slop, lock with own ticking, not from each methods
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

    async fn persist(&self) -> Result<()> {
        let mut signing = self.identity.signing_seed.unlock(|seed| encode(seed))?;
        let mut encryption = self
            .identity
            .x25519_secret
            .unlock(|secret| encode(secret))?;
        let persisted = PersistedConfig {
            version: CONFIG_VERSION,
            settings: self.settings.clone(),
            identity: PersistedIdentity {
                ed25519_signing_seed: std::mem::take(&mut signing),
                x25519_static_secret: std::mem::take(&mut encryption),
            },
            approved_client_bundles: self.approved_clients.clone(),
        };
        signing.zeroize();
        encryption.zeroize();
        let bytes = Zeroizing::new(serde_json::to_vec(&persisted)?);
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
