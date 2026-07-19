use std::fmt::Write;

use js_sys::Uint8Array;
use keeless_core::StorageProvider;
use keeless_sync::{
    ByteRange, FileMetadata, RemoteFile, Revision, StorageError, StorageErrorKind, StorageFuture,
    WriteCondition, WriteOutcome,
};
use sha2::{Digest, Sha256};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{File, FileSystemFileHandle, FileSystemWritableFileStream, WritableStream};

use crate::inclusive_range;

pub(crate) struct LocalFileStorage {
    pub(crate) file: File,
    pub(crate) handle: Option<FileSystemFileHandle>,
}

impl LocalFileStorage {
    async fn current_file(&self) -> Result<File, StorageError> {
        let Some(handle) = &self.handle else {
            return Ok(self.file.clone());
        };
        JsFuture::from(handle.get_file())
            .await
            .map_err(|error| file_error("read local file", error))?
            .dyn_into()
            .map_err(|_| storage_error(StorageErrorKind::Other, "invalid local file handle"))
    }
}

impl StorageProvider for LocalFileStorage {
    fn read<'a>(
        &'a self,
        _path: &'a str,
        range: Option<ByteRange>,
    ) -> StorageFuture<'a, Result<RemoteFile, StorageError>> {
        Box::pin(async move {
            let file = self.current_file().await?;
            let full_bytes = file_bytes(&file).await?;
            let metadata = metadata(&file, &full_bytes);
            let bytes = match range {
                Some(range) => inclusive_range(&full_bytes, range.start, range.end)
                    .map_err(|error| storage_error(StorageErrorKind::InvalidInput, error))?,
                None => full_bytes,
            };
            Ok(RemoteFile { bytes, metadata })
        })
    }

    fn stat<'a>(
        &'a self,
        _path: &'a str,
    ) -> StorageFuture<'a, Result<Option<FileMetadata>, StorageError>> {
        Box::pin(async move {
            let file = self.current_file().await?;
            let bytes = file_bytes(&file).await?;
            Ok(Some(metadata(&file, &bytes)))
        })
    }

    fn write<'a>(
        &'a self,
        _path: &'a str,
        bytes: Vec<u8>,
        condition: WriteCondition,
    ) -> StorageFuture<'a, Result<WriteOutcome, StorageError>> {
        Box::pin(async move {
            let handle = self.handle.as_ref().ok_or_else(|| {
                storage_error(
                    StorageErrorKind::Unsupported,
                    "this local file is read-only",
                )
            })?;
            let current = self.current_file().await?;
            let current_bytes = file_bytes(&current).await?;
            let current_revision = revision(&current_bytes);
            let matches = match condition {
                WriteCondition::Unconditional => true,
                WriteCondition::MustNotExist => false,
                WriteCondition::MustMatch(expected) => expected == current_revision,
            };
            if !matches {
                return Ok(WriteOutcome::Conflict);
            }

            let writable: FileSystemWritableFileStream = JsFuture::from(handle.create_writable())
                .await
                .map_err(|error| file_error("open local file for writing", error))?
                .dyn_into()
                .map_err(|_| storage_error(StorageErrorKind::Other, "invalid writable stream"))?;
            let write = writable
                .write_with_u8_array(&bytes)
                .map_err(|error| file_error("write local file", error))?;
            JsFuture::from(write)
                .await
                .map_err(|error| file_error("write local file", error))?;
            let truncate = writable
                .truncate_with_f64(bytes.len() as f64)
                .map_err(|error| file_error("truncate local file", error))?;
            JsFuture::from(truncate)
                .await
                .map_err(|error| file_error("truncate local file", error))?;
            JsFuture::from(WritableStream::from(writable).close())
                .await
                .map_err(|error| file_error("close local file", error))?;

            let committed = self.current_file().await?;
            let committed_bytes = file_bytes(&committed).await?;
            if committed_bytes != bytes {
                return Ok(WriteOutcome::Conflict);
            }
            Ok(WriteOutcome::Applied {
                revision: Some(revision(&committed_bytes)),
            })
        })
    }

    fn delete<'a>(&'a self, _path: &'a str) -> StorageFuture<'a, Result<(), StorageError>> {
        Box::pin(async {
            Err(storage_error(
                StorageErrorKind::Unsupported,
                "local file deletion is not supported",
            ))
        })
    }
}

async fn file_bytes(file: &File) -> Result<Vec<u8>, StorageError> {
    let buffer = JsFuture::from(file.array_buffer())
        .await
        .map_err(|error| file_error("read local file", error))?;
    Ok(Uint8Array::new(&buffer).to_vec())
}

fn metadata(file: &File, bytes: &[u8]) -> FileMetadata {
    FileMetadata {
        size: bytes.len() as u64,
        revision: Some(revision(bytes)),
        last_modified: Some(file.last_modified().trunc().to_string()),
    }
}

fn revision(bytes: &[u8]) -> Revision {
    let digest = Sha256::digest(bytes);
    let mut value = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(value, "{byte:02x}");
    }
    Revision::StrongEtag(format!("sha256:{value}"))
}

fn file_error(operation: &str, error: JsValue) -> StorageError {
    let name = error
        .dyn_ref::<web_sys::DomException>()
        .map(web_sys::DomException::name)
        .unwrap_or_default();
    let kind = match name.as_str() {
        "NotFoundError" => StorageErrorKind::NotFound,
        "NotAllowedError" | "NoModificationAllowedError" | "SecurityError" => {
            StorageErrorKind::PermissionDenied
        }
        "InvalidStateError" => StorageErrorKind::Conflict,
        "DataError" | "TypeMismatchError" => StorageErrorKind::InvalidInput,
        "NotSupportedError" => StorageErrorKind::Unsupported,
        _ => StorageErrorKind::Other,
    };
    storage_error(kind, operation)
}

fn storage_error(kind: StorageErrorKind, message: impl Into<String>) -> StorageError {
    StorageError::new(kind, message)
}
