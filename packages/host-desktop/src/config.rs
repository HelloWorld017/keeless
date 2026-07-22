use std::{io, path::PathBuf};

use directories::ProjectDirs;
use keeless_core::{ConfigProvider, CoreError, HostFuture};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const MAX_CONFIG_SIZE: usize = 1024 * 1024;
pub const CORE_SETTINGS_FILE: &str = "core-settings.json";
pub const WIRE_STATE_FILE: &str = "wire-state.json";

#[derive(Debug)]
pub struct DesktopConfig {
    path: PathBuf,
}

impl DesktopConfig {
    pub fn project(file_name: &str) -> io::Result<Self> {
        let dirs = ProjectDirs::from("dev", "nenw", "keeless")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no user config directory"))?;
        Ok(Self::at(dirs.data_dir().join(file_name)))
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn directory(&self) -> io::Result<&std::path::Path> {
        self.path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "config path has no parent"))
    }

    fn host_error(error: impl std::fmt::Display) -> CoreError {
        CoreError::Host(error.to_string())
    }
}

impl ConfigProvider for DesktopConfig {
    fn load(&self) -> HostFuture<'_, keeless_core::Result<Option<Vec<u8>>>> {
        Box::pin(async move {
            let file = match tokio::fs::File::open(&self.path).await {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(Self::host_error(error)),
            };
            let mut bytes = Vec::new();
            file.take((MAX_CONFIG_SIZE + 1) as u64)
                .read_to_end(&mut bytes)
                .await
                .map_err(Self::host_error)?;
            if bytes.len() > MAX_CONFIG_SIZE {
                return Err(CoreError::InvalidConfig(
                    "configuration is too large".into(),
                ));
            }
            Ok(Some(bytes))
        })
    }

    fn save<'a>(&'a self, config: &'a [u8]) -> HostFuture<'a, keeless_core::Result<()>> {
        Box::pin(async move {
            if config.len() > MAX_CONFIG_SIZE {
                return Err(CoreError::InvalidConfig(
                    "configuration is too large".into(),
                ));
            }
            let directory = self.directory().map_err(Self::host_error)?;
            tokio::fs::create_dir_all(directory)
                .await
                .map_err(Self::host_error)?;
            set_directory_permissions(directory).map_err(Self::host_error)?;

            let temporary = temporary_path(&self.path).map_err(Self::host_error)?;
            let mut options = tokio::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                options.mode(0o600);
            }
            let mut file = options.open(&temporary).await.map_err(Self::host_error)?;
            let result = async {
                file.write_all(config).await?;
                file.sync_all().await?;
                drop(file);
                replace_file(&temporary, &self.path)?;
                sync_directory(directory)?;
                Ok::<_, io::Error>(())
            }
            .await;
            if result.is_err() {
                let _ = tokio::fs::remove_file(&temporary).await;
            }
            result.map_err(Self::host_error)
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

pub(crate) fn temporary_path(target: &std::path::Path) -> io::Result<PathBuf> {
    let mut random = [0_u8; 16];
    getrandom::getrandom(&mut random).map_err(io::Error::other)?;
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no file name"))?;
    Ok(target.with_file_name(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        hex::encode(random)
    )))
}

#[cfg(unix)]
fn set_directory_permissions(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(windows)]
fn set_directory_permissions(_: &std::path::Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
pub(crate) fn replace_file(from: &std::path::Path, to: &std::path::Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

#[cfg(windows)]
pub(crate) fn replace_file(from: &std::path::Path, to: &std::path::Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn sync_directory(path: &std::path::Path) -> io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

#[cfg(windows)]
fn sync_directory(_: &std::path::Path) -> io::Result<()> {
    Ok(())
}

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
            std::fs::metadata(&provider.path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
