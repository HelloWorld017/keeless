use keeless_schema::OperationError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("host error: {0}")]
    Host(String),
    #[error("invalid persisted configuration: {0}")]
    InvalidConfig(String),
    #[error("unknown storage provider: {0}")]
    UnknownStorageProvider(String),
    #[error("no database is selected")]
    NoDatabaseSelected,
    #[error("database does not exist")]
    DatabaseNotFound,
    #[error("database is locked")]
    DatabaseLocked,
    #[error("invalid database credentials")]
    InvalidCredentials,
    #[error("password is required")]
    PasswordRequired,
    #[error("cryptographic operation failed")]
    Crypto,
    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error(transparent)]
    Storage(#[from] keeless_sync::StorageError),
    #[error(transparent)]
    Sync(keeless_sync::SyncError),
    #[error("secure memory operation failed")]
    SecureMemory,
}

impl From<keeless_secure_types::Error> for CoreError {
    fn from(_: keeless_secure_types::Error) -> Self {
        Self::SecureMemory
    }
}

impl From<keeless_kdbx::DatabaseError> for CoreError {
    fn from(error: keeless_kdbx::DatabaseError) -> Self {
        match error {
            keeless_kdbx::DatabaseError::InvalidCredentials => Self::InvalidCredentials,
            keeless_kdbx::DatabaseError::SecureMemory(_) => Self::SecureMemory,
            _ => Self::Crypto,
        }
    }
}

impl From<keeless_sync::SyncError> for CoreError {
    fn from(error: keeless_sync::SyncError) -> Self {
        match error {
            keeless_sync::SyncError::Database(keeless_kdbx::DatabaseError::InvalidCredentials) => {
                Self::InvalidCredentials
            }
            keeless_sync::SyncError::Database(keeless_kdbx::DatabaseError::SecureMemory(_)) => {
                Self::SecureMemory
            }
            keeless_sync::SyncError::RemoteNotFound(_) => Self::DatabaseNotFound,
            error => Self::Sync(error),
        }
    }
}

impl From<&CoreError> for OperationError {
    fn from(error: &CoreError) -> Self {
        let (code, message) = match error {
            CoreError::UnknownStorageProvider(_) => (
                "storage_provider_unknown",
                "Storage provider is unavailable",
            ),
            CoreError::NoDatabaseSelected => ("database_not_selected", "No database is selected"),
            CoreError::DatabaseNotFound => ("database_not_found", "Database does not exist"),
            CoreError::DatabaseLocked => ("database_locked", "Database is locked"),
            CoreError::InvalidCredentials => {
                ("invalid_credentials", "Database credentials are invalid")
            }
            CoreError::PasswordRequired => ("password_required", "Password is required"),
            CoreError::Storage(_) | CoreError::Sync(_) => {
                ("storage_error", "Storage operation failed")
            }
            _ => ("operation_failed", "Operation failed"),
        };
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, CoreError>;
