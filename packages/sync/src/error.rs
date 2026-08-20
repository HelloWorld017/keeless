use keeless_kdbx::DatabaseError;
use thiserror::Error;

/// Broad storage error categories callers can handle without parsing messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageErrorKind {
    NotFound,
    AlreadyExists,
    Authentication,
    PermissionDenied,
    Conflict,
    InvalidInput,
    Network,
    Unsupported,
    Server,
    Other,
}

/// Error returned by a storage provider.
#[derive(Debug, Error)]
#[error("{kind:?}: {message}")]
pub struct StorageError {
    kind: StorageErrorKind,
    message: String,
}

impl StorageError {
    pub fn new(kind: StorageErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> StorageErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Error returned while opening, creating, or synchronizing a KDBX file.
#[derive(Debug, Error)]
pub enum SyncError {
    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error(transparent)]
    Database(#[from] DatabaseError),

    #[error("remote file does not exist")]
    RemoteNotFound,

    #[error("credentials are required because the database KDF changed")]
    CredentialsRequired,

    #[error("storage does not provide an atomic revision")]
    AtomicUpdateUnsupported,

    #[error("remote database has a different root group ID")]
    RootGroupMismatch,

    #[error("conditional update did not succeed after {attempts} attempts")]
    RetryExhausted { attempts: usize },
}
