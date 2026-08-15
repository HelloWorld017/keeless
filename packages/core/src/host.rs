use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use keeless_sync::StorageProvider;
use zeroize::Zeroizing;

use crate::{DatabaseId, Result, StorageDescriptor};
use keeless_lesswire::{KeyScope, StateStore};

#[cfg(not(target_arch = "wasm32"))]
pub type HostFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(target_arch = "wasm32")]
pub type HostFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

#[cfg(not(target_arch = "wasm32"))]
pub trait HostProviderRequirements: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync + ?Sized> HostProviderRequirements for T {}

#[cfg(target_arch = "wasm32")]
pub trait HostProviderRequirements {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> HostProviderRequirements for T {}

/// Durable storage used for the local database cache, mutation journal, and encrypted Core state.
pub trait DatabasePersistence: HostProviderRequirements {
    /// Select the persistence namespace associated with a storage capability.
    fn select<'a>(
        &'a self,
        descriptor: &'a StorageDescriptor,
    ) -> HostFuture<'a, Result<DatabaseId>>;
    fn select_by_id<'a>(&'a self, database_id: &'a DatabaseId) -> HostFuture<'a, Result<()>>;
    /// Removes only Core's app-local persistence namespace.
    fn purge<'a>(&'a self, database_id: &'a DatabaseId) -> HostFuture<'a, Result<()>>;
    fn read_cache(&self) -> HostFuture<'_, Result<Option<Vec<u8>>>>;
    fn write_cache<'a>(&'a self, cache: &'a [u8]) -> HostFuture<'a, Result<()>>;
    fn read_journal(&self) -> HostFuture<'_, Result<Vec<Vec<u8>>>>;
    /// Return only after the complete line is durably appended.
    fn append_journal<'a>(&'a self, line: &'a [u8]) -> HostFuture<'a, Result<()>>;
    /// Durably remove all journal lines after their database state is persisted.
    fn clear_journal(&self) -> HostFuture<'_, Result<()>>;
    fn quarantine_cache<'a>(&'a self, reason: &'a str) -> HostFuture<'a, Result<()>>;
    fn quarantine_journal<'a>(&'a self, reason: &'a str) -> HostFuture<'a, Result<()>>;
    /// Reads a bounded Core-owned opaque record in the selected database namespace.
    fn read_state_record<'a>(&'a self, name: &'a str) -> HostFuture<'a, Result<Option<Vec<u8>>>>;
    /// Atomically replaces a bounded Core-owned opaque record in the selected namespace.
    fn write_state_record<'a>(
        &'a self,
        name: &'a str,
        bytes: &'a [u8],
    ) -> HostFuture<'a, Result<()>>;
}

/// Recreates host storage capabilities from encrypted database configuration.
pub trait StorageConfigurer: HostProviderRequirements {
    fn configure(
        &self,
        config: keeless_schema::DatabaseStorageConfig,
    ) -> HostFuture<'_, Result<(StorageDescriptor, Arc<dyn StorageProvider>)>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionApprovalKind {
    Initial,
    Upgrade,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionApprovalRequest {
    pub sender: String,
    pub sender_scope: KeyScope,
    pub recipient: String,
    pub recipient_scope: KeyScope,
    pub kind: ConnectionApprovalKind,
}

/// Requests user approval without exposing Core's decrypted database state to the host.
pub trait ConnectionApprovalProvider: HostProviderRequirements {
    fn approve_connection(
        &self,
        request: ConnectionApprovalRequest,
    ) -> HostFuture<'_, Result<bool>>;
}

/// Host-specific detached task execution used for storage fetches and maintenance work.
pub trait TaskSpawner: HostProviderRequirements {
    fn spawn(&self, task: HostFuture<'static, ()>);
}

/// Host-owned Lesswire transfer registry. Core only exchanges opaque IDs and owned bytes.
pub trait TransferProvider: HostProviderRequirements {
    fn publish_download(&self, owner: &str, bytes: Zeroizing<Vec<u8>>) -> Result<String>;
    fn consume_upload(&self, owner: &str, transfer_id: &str) -> Result<Zeroizing<Vec<u8>>>;
    fn clear(&self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordInputMode {
    Create,
    Unlock,
    Reveal,
    Save,
}

pub trait PasswordInputProvider: HostProviderRequirements {
    fn request_password(
        &self,
        mode: PasswordInputMode,
    ) -> HostFuture<'_, Result<Option<Zeroizing<Vec<u8>>>>>;
}

/// The user-presence ceremony Core asks the native host to present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasskeyConsentMode {
    Register,
    Assert,
}

/// Trusted context Core supplies for a passkey user-presence prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasskeyConsentRequest {
    pub mode: PasskeyConsentMode,
    pub rp_id: String,
    /// Account names in display order. The host returns an index into this list.
    pub accounts: Vec<String>,
}

pub trait PasskeyConsentProvider: HostProviderRequirements {
    /// Returns the selected account index, or `None` when the user declined.
    fn request_passkey_consent(
        &self,
        request: PasskeyConsentRequest,
    ) -> HostFuture<'_, Result<Option<usize>>>;
}

pub trait Clock: HostProviderRequirements {
    /// Unix time in milliseconds, used only for protocol timestamps.
    fn now_millis(&self) -> i64;

    /// Monotonic milliseconds from an arbitrary origin, used for elapsed durations.
    fn monotonic_millis(&self) -> u64;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};

        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        millis.min(i64::MAX as u128) as i64
    }

    fn monotonic_millis(&self) -> u64 {
        use std::sync::OnceLock;
        use std::time::Instant;

        static ORIGIN: OnceLock<Instant> = OnceLock::new();
        ORIGIN
            .get_or_init(Instant::now)
            .elapsed()
            .as_millis()
            .min(u64::MAX as u128) as u64
    }
}

pub struct KeelessHost {
    pub storage_providers: HashMap<String, Arc<dyn StorageProvider>>,
    /// Global plaintext state for the always-available untrusted Lesswire endpoint.
    pub untrusted_state: Arc<dyn StateStore>,
    /// Core-owned global plaintext state, separate from untrusted Lesswire state.
    pub core_state: Arc<dyn StateStore>,
    pub connection_approval: Arc<dyn ConnectionApprovalProvider>,
    pub password_input: Option<Arc<dyn PasswordInputProvider>>,
    pub passkey_consent: Option<Arc<dyn PasskeyConsentProvider>>,
    pub clock: Arc<dyn Clock>,
    pub database_persistence: Arc<dyn DatabasePersistence>,
    pub storage_configurer: Option<Arc<dyn StorageConfigurer>>,
    pub task_spawner: Option<Arc<dyn TaskSpawner>>,
    pub transfer_provider: Option<Arc<dyn TransferProvider>>,
}

impl std::fmt::Debug for KeelessHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeelessHost")
            .field("storage_provider_names", &self.storage_providers.keys())
            .field("has_password_input", &self.password_input.is_some())
            .field("has_connection_approval", &true)
            .field("has_passkey_consent", &self.passkey_consent.is_some())
            .field("has_database_persistence", &true)
            .field("has_storage_configurer", &self.storage_configurer.is_some())
            .field("has_task_spawner", &self.task_spawner.is_some())
            .field("has_transfer_provider", &self.transfer_provider.is_some())
            .finish_non_exhaustive()
    }
}
