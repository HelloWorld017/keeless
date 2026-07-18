use std::future::Future;
use std::pin::Pin;

use crate::StorageError;

#[cfg(not(target_arch = "wasm32"))]
pub type StorageFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(target_arch = "wasm32")]
pub type StorageFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Inclusive byte range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

impl ByteRange {
    pub fn new(start: u64, end: u64) -> Result<Self, StorageError> {
        if start > end {
            return Err(StorageError::new(
                crate::StorageErrorKind::InvalidInput,
                "byte range start must not exceed end",
            ));
        }
        Ok(Self { start, end })
    }
}

/// Opaque value suitable for a provider's conditional update operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revision {
    StrongEtag(String),
    LastModified(String),
}

/// Metadata observed for one representation of a remote file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    pub size: u64,
    pub revision: Option<Revision>,
    pub last_modified: Option<String>,
}

/// File contents and metadata obtained from the same storage read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    pub bytes: Vec<u8>,
    pub metadata: FileMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteCondition {
    Unconditional,
    MustNotExist,
    MustMatch(Revision),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome {
    Applied { revision: Option<Revision> },
    Conflict,
}

#[cfg(not(target_arch = "wasm32"))]
#[doc(hidden)]
pub trait StorageProviderRequirements: Send + Sync {}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync + ?Sized> StorageProviderRequirements for T {}

#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub trait StorageProviderRequirements {}

#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> StorageProviderRequirements for T {}

/// Object-safe asynchronous storage interface shared by native and WASM hosts.
pub trait StorageProvider: StorageProviderRequirements {
    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<ByteRange>,
    ) -> StorageFuture<'a, Result<RemoteFile, StorageError>>;

    fn stat<'a>(
        &'a self,
        path: &'a str,
    ) -> StorageFuture<'a, Result<Option<FileMetadata>, StorageError>>;

    fn write<'a>(
        &'a self,
        path: &'a str,
        bytes: Vec<u8>,
        condition: WriteCondition,
    ) -> StorageFuture<'a, Result<WriteOutcome, StorageError>>;

    fn delete<'a>(&'a self, path: &'a str) -> StorageFuture<'a, Result<(), StorageError>>;
}
