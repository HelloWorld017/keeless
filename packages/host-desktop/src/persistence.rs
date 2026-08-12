use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use directories::ProjectDirs;
use keeless_core::{
    CoreError, DatabaseId, DatabasePersistence, HostFuture, Result, StorageDescriptor,
};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use keeless_host_desktop_shared::fs::{
    replace_file, set_directory_permissions, set_file_permissions, set_private_create_mode,
    sync_directory, temporary_path,
};

use crate::storage::LocalFileStorage;

pub const MAX_CACHE_SIZE: usize = 128 * 1024 * 1024 + 64;
pub const MAX_JOURNAL_SIZE: usize = 16 * 1024 * 1024;
pub const MAX_STATE_RECORD_SIZE: usize = 128 * 1024;

pub fn database_id_from_backing_path(provider: &str, path: &Path) -> io::Result<DatabaseId> {
    if provider.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "database provider is empty",
        ));
    }
    let canonical = canonical_backing_path(path)?;
    let mut digest = Sha256::new();
    digest.update((provider.len() as u64).to_be_bytes());
    digest.update(provider.as_bytes());
    update_path_digest(&mut digest, canonical.as_os_str());
    Ok(DatabaseId::new(
        URL_SAFE_NO_PAD.encode(digest.finalize()).into_bytes(),
    ))
}

/// Canonicalizes an existing backing file, or its parent when the selected create target is absent.
pub fn canonical_backing_path(path: &Path) -> io::Result<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let file_name = path.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "backing path has no file name")
            })?;
            let parent = path.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "backing path has no parent")
            })?;
            Ok(std::fs::canonicalize(parent)?.join(file_name))
        }
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn update_path_digest(digest: &mut Sha256, path: &OsStr) {
    use std::os::unix::ffi::OsStrExt;
    digest.update(path.as_bytes());
}

#[cfg(windows)]
fn update_path_digest(digest: &mut Sha256, path: &OsStr) {
    use std::os::windows::ffi::OsStrExt;
    for unit in path.encode_wide() {
        digest.update(unit.to_le_bytes());
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistenceFile {
    Cache,
    Journal,
    State(&'static str),
}

impl PersistenceFile {
    fn name(self) -> &'static str {
        match self {
            Self::Cache => "cache",
            Self::Journal => "journal",
            Self::State(name) => name,
        }
    }
}

#[derive(Debug)]
struct DatabaseStore {
    directory: PathBuf,
    writes: Arc<tokio::sync::Mutex<()>>,
}

impl DatabaseStore {
    fn at(
        app_data: impl AsRef<Path>,
        database_id: &DatabaseId,
        writes: Arc<tokio::sync::Mutex<()>>,
    ) -> Self {
        Self {
            directory: app_data.as_ref().join(format!(
                "db_{}",
                std::str::from_utf8(database_id.as_bytes()).expect("desktop database ID is ASCII")
            )),
            writes,
        }
    }

    #[cfg(test)]
    fn directory(&self) -> &Path {
        &self.directory
    }

    fn cache_path(&self) -> PathBuf {
        self.path(PersistenceFile::Cache)
    }

    fn journal_path(&self) -> PathBuf {
        self.path(PersistenceFile::Journal)
    }

    async fn read_cache(&self) -> io::Result<Option<Vec<u8>>> {
        read_bounded(&self.cache_path(), MAX_CACHE_SIZE).await
    }

    async fn replace_cache(&self, bytes: &[u8]) -> io::Result<()> {
        ensure_bound(bytes.len(), MAX_CACHE_SIZE, "cache")?;
        let _guard = self.writes.lock().await;
        atomic_replace(&self.cache_path(), bytes).await
    }

    async fn read_journal(&self) -> io::Result<Option<Vec<u8>>> {
        read_bounded(&self.journal_path(), MAX_JOURNAL_SIZE).await
    }

    async fn append_journal_line(&self, line: &[u8]) -> io::Result<()> {
        if line.contains(&b'\n') || line.contains(&b'\r') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "journal entry must be one line",
            ));
        }
        let _guard = self.writes.lock().await;
        prepare_directory(&self.directory).await?;
        let path = self.journal_path();
        let current_size = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error),
        };
        if line.len() >= MAX_JOURNAL_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "journal entry is too large",
            ));
        }
        let new_size = current_size
            .checked_add((line.len() + 1) as u64)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "journal is too large"))?;
        if new_size > MAX_JOURNAL_SIZE as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "journal is too large",
            ));
        }

        let mut options = tokio::fs::OpenOptions::new();
        options.create(true).append(true);
        set_private_create_mode(&mut options);
        let mut file = options.open(path).await?;
        set_file_permissions(&file).await?;
        file.write_all(line).await?;
        file.write_all(b"\n").await?;
        file.sync_all().await?;
        sync_directory(&self.directory)
    }

    async fn replace_journal(&self, bytes: &[u8]) -> io::Result<()> {
        ensure_bound(bytes.len(), MAX_JOURNAL_SIZE, "journal")?;
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "journal must end at a line boundary",
            ));
        }
        let _guard = self.writes.lock().await;
        atomic_replace(&self.journal_path(), bytes).await
    }

    async fn clear_journal(&self) -> io::Result<()> {
        self.replace_journal(&[]).await
    }

    async fn read_state_record(&self, name: &'static str) -> io::Result<Option<Vec<u8>>> {
        read_bounded(
            &self.path(PersistenceFile::State(name)),
            MAX_STATE_RECORD_SIZE,
        )
        .await
    }

    async fn replace_state_record(&self, name: &'static str, bytes: &[u8]) -> io::Result<()> {
        ensure_bound(bytes.len(), MAX_STATE_RECORD_SIZE, "state record")?;
        let _guard = self.writes.lock().await;
        atomic_replace(&self.path(PersistenceFile::State(name)), bytes).await
    }

    /// Moves bytes rejected by a decoder aside and returns the quarantine path.
    async fn quarantine_corrupt(&self, file: PersistenceFile) -> io::Result<Option<PathBuf>> {
        let _guard = self.writes.lock().await;
        let source = self.path(file);
        let quarantine = corrupt_path(&source)?;
        match tokio::fs::rename(&source, &quarantine).await {
            Ok(()) => {
                sync_directory(&self.directory)?;
                Ok(Some(quarantine))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn path(&self, file: PersistenceFile) -> PathBuf {
        self.directory.join(file.name())
    }
}

#[derive(Debug)]
pub struct DesktopDatabasePersistence {
    app_data: PathBuf,
    storage: Arc<LocalFileStorage>,
    selected: tokio::sync::RwLock<Option<DatabaseId>>,
    writes: Arc<tokio::sync::Mutex<()>>,
}

impl DesktopDatabasePersistence {
    pub fn project(storage: Arc<LocalFileStorage>) -> io::Result<Self> {
        let dirs = ProjectDirs::from("dev", "nenw", "keeless")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no user data directory"))?;
        Ok(Self::at(dirs.data_dir(), storage))
    }

    pub fn at(app_data: impl AsRef<Path>, storage: Arc<LocalFileStorage>) -> Self {
        Self {
            app_data: app_data.as_ref().to_path_buf(),
            storage,
            selected: tokio::sync::RwLock::new(None),
            writes: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    async fn store(&self) -> Result<DatabaseStore> {
        let database_id = self
            .selected
            .read()
            .await
            .clone()
            .ok_or_else(|| CoreError::Host("database persistence is not selected".into()))?;
        Ok(DatabaseStore::at(
            &self.app_data,
            &database_id,
            Arc::clone(&self.writes),
        ))
    }
}

impl DatabasePersistence for DesktopDatabasePersistence {
    fn select<'a>(
        &'a self,
        descriptor: &'a StorageDescriptor,
    ) -> HostFuture<'a, Result<DatabaseId>> {
        Box::pin(async move {
            if descriptor.provider != "local-file" {
                return Err(CoreError::Host(
                    "desktop persistence requires local-file storage".into(),
                ));
            }
            let database_id = self
                .storage
                .resolve_capability(&descriptor.path)?
                .database_id()
                .clone();
            *self.selected.write().await = Some(database_id.clone());
            Ok(database_id)
        })
    }

    fn read_cache(&self) -> HostFuture<'_, Result<Option<Vec<u8>>>> {
        Box::pin(async move { self.store().await?.read_cache().await.map_err(host_error) })
    }

    fn write_cache<'a>(&'a self, cache: &'a [u8]) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            self.store()
                .await?
                .replace_cache(cache)
                .await
                .map_err(host_error)
        })
    }

    fn read_journal(&self) -> HostFuture<'_, Result<Vec<Vec<u8>>>> {
        Box::pin(async move {
            let Some(bytes) = self
                .store()
                .await?
                .read_journal()
                .await
                .map_err(host_error)?
            else {
                return Ok(Vec::new());
            };
            if bytes.is_empty() {
                return Ok(Vec::new());
            }
            let lines = bytes.strip_suffix(b"\n").ok_or_else(|| {
                CoreError::Host("persisted journal does not end at a line boundary".into())
            })?;
            Ok(lines.split(|byte| *byte == b'\n').map(Vec::from).collect())
        })
    }

    fn append_journal<'a>(&'a self, line: &'a [u8]) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            self.store()
                .await?
                .append_journal_line(line)
                .await
                .map_err(host_error)
        })
    }

    fn clear_journal(&self) -> HostFuture<'_, Result<()>> {
        Box::pin(async move {
            self.store()
                .await?
                .clear_journal()
                .await
                .map_err(host_error)
        })
    }

    fn quarantine_cache<'a>(&'a self, _reason: &'a str) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            self.store()
                .await?
                .quarantine_corrupt(PersistenceFile::Cache)
                .await
                .map(|_| ())
                .map_err(host_error)
        })
    }

    fn quarantine_journal<'a>(&'a self, _reason: &'a str) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            self.store()
                .await?
                .quarantine_corrupt(PersistenceFile::Journal)
                .await
                .map(|_| ())
                .map_err(host_error)
        })
    }

    fn read_state_record<'a>(&'a self, name: &'a str) -> HostFuture<'a, Result<Option<Vec<u8>>>> {
        Box::pin(async move {
            let name = state_record_name(name)?;
            self.store()
                .await?
                .read_state_record(name)
                .await
                .map_err(host_error)
        })
    }

    fn write_state_record<'a>(
        &'a self,
        name: &'a str,
        bytes: &'a [u8],
    ) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            let name = state_record_name(name)?;
            self.store()
                .await?
                .replace_state_record(name, bytes)
                .await
                .map_err(host_error)
        })
    }
}

fn state_record_name(name: &str) -> Result<&'static str> {
    match name {
        "config" => Ok("state-config"),
        "core-wire-state" => Ok("state-core-wire"),
        _ => Err(CoreError::Host("invalid database state record name".into())),
    }
}

fn host_error(error: io::Error) -> CoreError {
    CoreError::Host(error.to_string())
}

async fn read_bounded(path: &Path, maximum: usize) -> io::Result<Option<Vec<u8>>> {
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if file.metadata().await?.len() > maximum as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "persistence file is too large",
        ));
    }
    let mut bytes = Vec::new();
    file.take((maximum + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "persistence file is too large",
        ));
    }
    Ok(Some(bytes))
}

async fn atomic_replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "persistence path has no parent",
        )
    })?;
    prepare_directory(directory).await?;
    let temporary = temporary_path(path)?;
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    set_private_create_mode(&mut options);
    let mut file = options.open(&temporary).await?;
    let result = async {
        file.write_all(bytes).await?;
        file.sync_all().await?;
        drop(file);
        replace_file(&temporary, path)?;
        sync_directory(directory)
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    result
}

async fn prepare_directory(path: &Path) -> io::Result<()> {
    tokio::fs::create_dir_all(path).await?;
    set_directory_permissions(path)
}

fn ensure_bound(size: usize, maximum: usize, name: &str) -> io::Result<()> {
    if size > maximum {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} is too large"),
        ))
    } else {
        Ok(())
    }
}

fn corrupt_path(target: &Path) -> io::Result<PathBuf> {
    let mut random = [0_u8; 8];
    getrandom::getrandom(&mut random).map_err(io::Error::other)?;
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no file name"))?;
    Ok(target.with_file_name(format!(
        "{}.corrupt.{}",
        name.to_string_lossy(),
        hex::encode(random)
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_id_is_stable_for_a_create_target_and_separates_paths() {
        let directory = tempfile::tempdir().unwrap();
        let first_path = directory.path().join("first.kdbx");
        let second_path = directory.path().join("second.kdbx");
        let before = database_id_from_backing_path("local-file", &first_path).unwrap();
        std::fs::write(&first_path, b"database").unwrap();
        let after = database_id_from_backing_path("local-file", &first_path).unwrap();
        let other = database_id_from_backing_path("local-file", &second_path).unwrap();
        assert_eq!(before, after);
        assert_ne!(before, other);
        assert_eq!(before.as_bytes().len(), 43);
        assert!(!before.as_bytes().contains(&b'='));
    }

    #[tokio::test]
    async fn cache_and_journal_round_trip_and_quarantine() {
        let directory = tempfile::tempdir().unwrap();
        let backing = directory.path().join("vault.kdbx");
        let database_id = database_id_from_backing_path("local-file", &backing).unwrap();
        let store = DatabaseStore::at(
            directory.path().join("data"),
            &database_id,
            Arc::new(tokio::sync::Mutex::new(())),
        );

        assert_eq!(store.read_cache().await.unwrap(), None);
        store.replace_cache(b"cache-one").await.unwrap();
        store.replace_cache(b"cache-two").await.unwrap();
        assert_eq!(store.read_cache().await.unwrap().unwrap(), b"cache-two");
        store.append_journal_line(b"one").await.unwrap();
        store.append_journal_line(b"two").await.unwrap();
        assert_eq!(store.read_journal().await.unwrap().unwrap(), b"one\ntwo\n");
        store.replace_journal(b"three\n").await.unwrap();
        assert_eq!(store.read_journal().await.unwrap().unwrap(), b"three\n");
        store.clear_journal().await.unwrap();
        assert_eq!(store.read_journal().await.unwrap().unwrap(), b"");

        let quarantine = store
            .quarantine_corrupt(PersistenceFile::Cache)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(std::fs::read(quarantine).unwrap(), b"cache-two");
        assert_eq!(store.read_cache().await.unwrap(), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn persistence_files_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let database_id =
            database_id_from_backing_path("local-file", &directory.path().join("vault.kdbx"))
                .unwrap();
        let store = DatabaseStore::at(
            directory.path().join("data"),
            &database_id,
            Arc::new(tokio::sync::Mutex::new(())),
        );
        store.replace_cache(b"secret").await.unwrap();
        store.append_journal_line(b"entry").await.unwrap();

        assert_eq!(
            std::fs::metadata(store.directory())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(store.cache_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(store.journal_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
