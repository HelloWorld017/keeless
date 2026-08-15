//! Storage-provider abstractions and conflict-safe KDBX synchronization.

mod error;
mod storage;
mod sync;
mod webdav;

pub use error::{StorageError, StorageErrorKind, SyncError};
pub use storage::{
    ByteRange, FileMetadata, RemoteFile, Revision, StorageFuture, StorageProvider, WriteCondition,
    WriteOutcome,
};
pub use sync::{FileHandle, RetryPolicy, SyncOptions, SyncReport};
pub use webdav::WebDavProvider;
