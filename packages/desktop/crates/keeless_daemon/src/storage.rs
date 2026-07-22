use std::{collections::HashMap, fmt::Write as _, io, path::PathBuf, sync::RwLock};

use keeless_sync::{
    ByteRange, FileMetadata, RemoteFile, Revision, StorageError, StorageErrorKind, StorageFuture,
    StorageProvider, WriteCondition, WriteOutcome,
};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::config::{replace_file, temporary_path};

pub const MAX_LOCAL_FILE_SIZE: u64 = 128 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct LocalFileStorage {
    capabilities: RwLock<HashMap<String, PathBuf>>,
    writes: tokio::sync::Mutex<()>,
}

impl LocalFileStorage {
    pub fn new() -> Self {
        Self::default()
    }

    /// The daemon calls this only with a path returned by the native picker.
    pub fn grant_picker_path(&self, path: PathBuf) -> io::Result<String> {
        let mut random = [0_u8; 32];
        getrandom::getrandom(&mut random).map_err(io::Error::other)?;
        let token = hex::encode(random);
        self.capabilities
            .write()
            .unwrap()
            .insert(token.clone(), path);
        Ok(token)
    }

    fn resolve(&self, token: &str) -> Result<PathBuf, StorageError> {
        self.capabilities
            .read()
            .unwrap()
            .get(token)
            .cloned()
            .ok_or_else(|| {
                error(
                    StorageErrorKind::PermissionDenied,
                    "invalid local-file capability",
                )
            })
    }
}

impl StorageProvider for LocalFileStorage {
    fn read<'a>(
        &'a self,
        token: &'a str,
        range: Option<ByteRange>,
    ) -> StorageFuture<'a, Result<RemoteFile, StorageError>> {
        Box::pin(async move {
            let path = self.resolve(token)?;
            let mut file = tokio::fs::File::open(&path)
                .await
                .map_err(|value| io_error("open local file", value))?;
            let size = file
                .metadata()
                .await
                .map_err(|value| io_error("stat local file", value))?
                .len();
            ensure_size(size)?;
            let mut all = Vec::with_capacity(size as usize);
            file.read_to_end(&mut all)
                .await
                .map_err(|value| io_error("read local file", value))?;
            let metadata = metadata(&all);
            let bytes = if let Some(range) = range {
                if range.start > range.end || range.end >= size {
                    return Err(error(
                        StorageErrorKind::InvalidInput,
                        "range is outside the file",
                    ));
                }
                all[range.start as usize..=range.end as usize].to_vec()
            } else {
                all
            };
            Ok(RemoteFile { bytes, metadata })
        })
    }

    fn stat<'a>(
        &'a self,
        token: &'a str,
    ) -> StorageFuture<'a, Result<Option<FileMetadata>, StorageError>> {
        Box::pin(async move {
            let path = self.resolve(token)?;
            let bytes = match read_all(&path).await {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == StorageErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error),
            };
            Ok(Some(metadata(&bytes)))
        })
    }

    fn write<'a>(
        &'a self,
        token: &'a str,
        bytes: Vec<u8>,
        condition: WriteCondition,
    ) -> StorageFuture<'a, Result<WriteOutcome, StorageError>> {
        Box::pin(async move {
            ensure_size(bytes.len() as u64)?;
            let path = self.resolve(token)?;
            let _guard = self.writes.lock().await;
            let must_not_exist = matches!(&condition, WriteCondition::MustNotExist);
            let current = match read_all(&path).await {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == StorageErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            let allowed = match condition {
                WriteCondition::Unconditional => true,
                WriteCondition::MustNotExist => current.is_none(),
                WriteCondition::MustMatch(expected) => current
                    .as_deref()
                    .is_some_and(|bytes| revision(bytes) == expected),
            };
            if !allowed {
                return Ok(WriteOutcome::Conflict);
            }

            let temporary = temporary_path(&path)
                .map_err(|value| io_error("create temporary file name", value))?;
            let mut options = tokio::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                options.mode(0o600);
            }
            let mut file = options
                .open(&temporary)
                .await
                .map_err(|value| io_error("create temporary local file", value))?;
            let write_result = async {
                file.write_all(&bytes).await?;
                file.sync_all().await?;
                drop(file);
                Ok::<_, io::Error>(())
            }
            .await;
            if let Err(value) = write_result {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(io_error("write temporary local file", value));
            }
            if must_not_exist {
                match tokio::fs::hard_link(&temporary, &path).await {
                    Ok(()) => {
                        let _ = tokio::fs::remove_file(&temporary).await;
                    }
                    Err(value) if value.kind() == io::ErrorKind::AlreadyExists => {
                        let _ = tokio::fs::remove_file(&temporary).await;
                        return Ok(WriteOutcome::Conflict);
                    }
                    Err(value) => {
                        let _ = tokio::fs::remove_file(&temporary).await;
                        return Err(io_error("create local file", value));
                    }
                }
            } else if let Err(value) = replace_file(&temporary, &path) {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(io_error("replace local file", value));
            }
            Ok(WriteOutcome::Applied {
                revision: Some(revision(&bytes)),
            })
        })
    }

    fn delete<'a>(&'a self, token: &'a str) -> StorageFuture<'a, Result<(), StorageError>> {
        Box::pin(async move {
            let path = self.resolve(token)?;
            let _guard = self.writes.lock().await;
            match tokio::fs::remove_file(path).await {
                Ok(()) => Ok(()),
                Err(value) if value.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(value) => Err(io_error("delete local file", value)),
            }
        })
    }
}

async fn read_all(path: &std::path::Path) -> Result<Vec<u8>, StorageError> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|value| io_error("open local file", value))?;
    let size = file
        .metadata()
        .await
        .map_err(|value| io_error("stat local file", value))?
        .len();
    ensure_size(size)?;
    let mut bytes = Vec::with_capacity(size as usize);
    file.seek(std::io::SeekFrom::Start(0))
        .await
        .map_err(|value| io_error("seek local file", value))?;
    file.read_to_end(&mut bytes)
        .await
        .map_err(|value| io_error("read local file", value))?;
    Ok(bytes)
}

fn ensure_size(size: u64) -> Result<(), StorageError> {
    if size > MAX_LOCAL_FILE_SIZE {
        Err(error(
            StorageErrorKind::InvalidInput,
            "local file is too large",
        ))
    } else {
        Ok(())
    }
}

fn metadata(bytes: &[u8]) -> FileMetadata {
    FileMetadata {
        size: bytes.len() as u64,
        revision: Some(revision(bytes)),
        last_modified: None,
    }
}

fn revision(bytes: &[u8]) -> Revision {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(7 + digest.len() * 2);
    encoded.push_str("sha256:");
    for byte in digest {
        let _ = write!(encoded, "{byte:02x}");
    }
    Revision::StrongEtag(encoded)
}

fn io_error(operation: &str, value: io::Error) -> StorageError {
    let kind = match value.kind() {
        io::ErrorKind::NotFound => StorageErrorKind::NotFound,
        io::ErrorKind::AlreadyExists => StorageErrorKind::AlreadyExists,
        io::ErrorKind::PermissionDenied => StorageErrorKind::PermissionDenied,
        io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => StorageErrorKind::InvalidInput,
        io::ErrorKind::Unsupported => StorageErrorKind::Unsupported,
        _ => StorageErrorKind::Other,
    };
    error(kind, format!("{operation}: {value}"))
}

fn error(kind: StorageErrorKind, message: impl Into<String>) -> StorageError {
    StorageError::new(kind, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_with_path(path: PathBuf) -> (LocalFileStorage, String) {
        let provider = LocalFileStorage::new();
        let token = provider.grant_picker_path(path).unwrap();
        (provider, token)
    }

    #[tokio::test]
    async fn rejects_unknown_capabilities() {
        let provider = LocalFileStorage::new();
        let error = provider.stat("/tmp/not-a-token").await.unwrap_err();
        assert_eq!(error.kind(), StorageErrorKind::PermissionDenied);
    }

    #[tokio::test]
    async fn reads_inclusive_ranges_and_rejects_invalid_ranges() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vault.kdbx");
        std::fs::write(&path, b"abcdef").unwrap();
        let (provider, token) = provider_with_path(path);
        let range = ByteRange::new(1, 3).unwrap();
        assert_eq!(
            provider.read(&token, Some(range)).await.unwrap().bytes,
            b"bcd"
        );
        let outside = ByteRange::new(1, 9).unwrap();
        assert_eq!(
            provider
                .read(&token, Some(outside))
                .await
                .unwrap_err()
                .kind(),
            StorageErrorKind::InvalidInput
        );
    }

    #[tokio::test]
    async fn enforces_create_and_revision_cas() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vault.kdbx");
        let (provider, token) = provider_with_path(path);
        let created = provider
            .write(&token, b"one".to_vec(), WriteCondition::MustNotExist)
            .await
            .unwrap();
        let revision = match created {
            WriteOutcome::Applied {
                revision: Some(revision),
            } => revision,
            _ => panic!("expected revision"),
        };
        assert_eq!(
            provider
                .write(&token, b"bad".to_vec(), WriteCondition::MustNotExist)
                .await
                .unwrap(),
            WriteOutcome::Conflict
        );
        assert!(matches!(
            provider
                .write(&token, b"two".to_vec(), WriteCondition::MustMatch(revision))
                .await
                .unwrap(),
            WriteOutcome::Applied { .. }
        ));
        assert_eq!(
            std::fs::read(provider.resolve(&token).unwrap()).unwrap(),
            b"two"
        );
    }

    #[tokio::test]
    async fn stale_revision_conflicts_and_delete_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vault.kdbx");
        std::fs::write(&path, b"current").unwrap();
        let (provider, token) = provider_with_path(path);
        let stale = revision(b"stale");
        assert_eq!(
            provider
                .write(&token, b"new".to_vec(), WriteCondition::MustMatch(stale))
                .await
                .unwrap(),
            WriteOutcome::Conflict
        );
        provider.delete(&token).await.unwrap();
        provider.delete(&token).await.unwrap();
    }
}
