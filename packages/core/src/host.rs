use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use keeless_sync::StorageProvider;

use crate::Result;

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

pub trait ConfigProvider: HostProviderRequirements {
    fn load(&self) -> HostFuture<'_, Result<Option<Vec<u8>>>>;
    /// Persist the versioned core configuration, which contains private identity keys.
    fn save<'a>(&'a self, config: &'a [u8]) -> HostFuture<'a, Result<()>>;
}

pub trait ClientApprovalProvider: HostProviderRequirements {
    fn approve(&self, public_key_bundle: &str) -> HostFuture<'_, Result<bool>>;
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
    pub default_approved_keys: Vec<String>,
    pub storage_providers: HashMap<String, Arc<dyn StorageProvider>>,
    pub config_provider: Arc<dyn ConfigProvider>,
    pub approval_provider: Arc<dyn ClientApprovalProvider>,
    pub clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for KeelessHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeelessHost")
            .field("default_approved_keys", &self.default_approved_keys)
            .field("storage_provider_names", &self.storage_providers.keys())
            .finish_non_exhaustive()
    }
}
