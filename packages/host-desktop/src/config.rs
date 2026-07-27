use std::{io, path::PathBuf};

use directories::ProjectDirs;
use keeless_core::{ConfigProvider, CoreError, HostFuture};
use keeless_host_desktop_shared::{fs, state::FileStore};

pub const MAX_CONFIG_SIZE: usize = 1024 * 1024;
pub const CORE_SETTINGS_FILE: &str = "core-settings.json";
pub const WIRE_STATE_FILE: &str = "wire-state.json";
pub const DESKTOP_WIRE_STATE_FILE: &str = "desktop-wire-state.json";

#[derive(Debug)]
pub struct DesktopConfig {
    store: FileStore,
}

impl DesktopConfig {
    pub fn project(file_name: &str) -> io::Result<Self> {
        let dirs = ProjectDirs::from("dev", "nenw", "keeless")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no user config directory"))?;
        Ok(Self::at(dirs.data_dir().join(file_name)))
    }

    pub fn at(path: PathBuf) -> Self {
        Self {
            store: FileStore::at(path),
        }
    }

    pub fn directory(&self) -> io::Result<&std::path::Path> {
        self.store.directory()
    }

    fn host_error(error: impl std::fmt::Display) -> CoreError {
        CoreError::Host(error.to_string())
    }
}

impl ConfigProvider for DesktopConfig {
    fn load(&self) -> HostFuture<'_, keeless_core::Result<Option<Vec<u8>>>> {
        Box::pin(async move {
            self.store
                .load(MAX_CONFIG_SIZE)
                .await
                .map(|config| config.map(|bytes| bytes.to_vec()))
                .map_err(|error| {
                    if error.kind() == io::ErrorKind::InvalidData {
                        CoreError::InvalidConfig("configuration is too large".into())
                    } else {
                        Self::host_error(error)
                    }
                })
        })
    }

    fn save<'a>(&'a self, config: &'a [u8]) -> HostFuture<'a, keeless_core::Result<()>> {
        Box::pin(async move {
            if config.len() > MAX_CONFIG_SIZE {
                return Err(CoreError::InvalidConfig(
                    "configuration is too large".into(),
                ));
            }
            self.store.save(config).await.map_err(Self::host_error)
        })
    }
}

impl keeless_lesswire::StateStore for DesktopConfig {
    fn load(&self) -> keeless_lesswire::WireFuture<'_, keeless_lesswire::Result<Option<Vec<u8>>>> {
        Box::pin(async move {
            ConfigProvider::load(self)
                .await
                .map_err(|error| keeless_lesswire::Error::Host(error.to_string()))
        })
    }

    fn save<'a>(
        &'a self,
        state: &'a [u8],
    ) -> keeless_lesswire::WireFuture<'a, keeless_lesswire::Result<()>> {
        Box::pin(async move {
            ConfigProvider::save(self, state)
                .await
                .map_err(|error| keeless_lesswire::Error::Host(error.to_string()))
        })
    }
}

pub(crate) use fs::{replace_file, temporary_path};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[tokio::test]
    async fn absent_config_and_atomic_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let provider = DesktopConfig::at(directory.path().join("nested/config.json"));
        assert_eq!(provider.load().await.unwrap(), None);
        provider.save(b"first").await.unwrap();
        provider.save(b"second").await.unwrap();
        assert_eq!(provider.load().await.unwrap(), Some(b"second".to_vec()));
        let entries = std::fs::read_dir(provider.directory().unwrap())
            .unwrap()
            .count();
        assert_eq!(entries, 1);
    }

    #[test]
    fn core_and_wire_use_distinct_new_file_names() {
        assert_eq!(CORE_SETTINGS_FILE, "core-settings.json");
        assert_eq!(WIRE_STATE_FILE, "wire-state.json");
        assert_ne!(CORE_SETTINGS_FILE, WIRE_STATE_FILE);
        assert!(![CORE_SETTINGS_FILE, WIRE_STATE_FILE].contains(&"core-config.json"));
        assert_ne!(DESKTOP_WIRE_STATE_FILE, WIRE_STATE_FILE);
    }

    #[tokio::test]
    async fn rejects_oversized_load_and_save() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        std::fs::write(&path, vec![0; MAX_CONFIG_SIZE + 1]).unwrap();
        let provider = Arc::new(DesktopConfig::at(path));
        assert!(provider.load().await.is_err());
        assert!(provider.save(&vec![0; MAX_CONFIG_SIZE + 1]).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn protects_config_directory_and_file() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let provider = DesktopConfig::at(directory.path().join("private/config.json"));
        provider.save(b"secret").await.unwrap();
        assert_eq!(
            std::fs::metadata(provider.directory().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(provider.store.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
