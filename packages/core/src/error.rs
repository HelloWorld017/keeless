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
    #[error("database already exists")]
    DatabaseAlreadyExists,
    #[error("database is locked")]
    DatabaseLocked,
    #[error("invalid database credentials")]
    InvalidCredentials,
    #[error("invalid source database credentials")]
    InvalidSourceCredentials,
    #[error("password is required")]
    PasswordRequired,
    #[error("group does not exist")]
    GroupNotFound,
    #[error("entry does not exist")]
    EntryNotFound,
    #[error("entry attachment does not exist")]
    AttachmentNotFound,
    #[error("invalid database node identifier")]
    InvalidNodeId,
    #[error("group cannot be moved to the requested location")]
    InvalidGroupMove,
    #[error("group cannot be deleted")]
    InvalidGroupDelete,
    #[error("entry cannot be moved to the requested location")]
    InvalidEntryMove,
    #[error("group name cannot be empty")]
    InvalidGroupName,
    #[error("entry field is invalid or is not protected")]
    InvalidEntryField,
    #[error("entry update is invalid")]
    InvalidEntryUpdate,
    #[error("entry cannot be deleted in the requested mode")]
    InvalidEntryDelete,
    #[error("icon reference is invalid")]
    InvalidIconReference,
    #[error("tag name cannot be empty")]
    InvalidTagName,
    #[error("tag style is invalid")]
    InvalidTagStyle,
    #[error("persisted tag styles are malformed")]
    MalformedTagStyles,
    #[error("tag style does not exist")]
    TagStyleNotFound,
    #[error("tag is used by an entry")]
    TagInUse,
    #[error("passkey request is invalid: {0}")]
    InvalidPasskeyRequest(keeless_kdbx::PasskeyError),
    #[error("passkey algorithm is not supported")]
    PasskeyUnsupportedAlgorithm,
    #[error("an excluded passkey already exists")]
    PasskeyExcluded,
    #[error("passkey does not exist")]
    PasskeyNotFound,
    #[error("cryptographic operation failed")]
    Crypto,
    #[error("mutation journal is invalid")]
    InvalidJournal,
    #[error("database cache is invalid")]
    InvalidCache,
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

impl From<keeless_kdbx::PasskeyError> for CoreError {
    fn from(error: keeless_kdbx::PasskeyError) -> Self {
        match error {
            keeless_kdbx::PasskeyError::UnsupportedAlgorithm => Self::PasskeyUnsupportedAlgorithm,
            keeless_kdbx::PasskeyError::CredentialExcluded => Self::PasskeyExcluded,
            keeless_kdbx::PasskeyError::CredentialNotAllowed
            | keeless_kdbx::PasskeyError::RpIdMismatch => Self::PasskeyNotFound,
            keeless_kdbx::PasskeyError::ProtectedFieldAccess => Self::Crypto,
            error => Self::InvalidPasskeyRequest(error),
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
            CoreError::DatabaseAlreadyExists => {
                ("database_already_exists", "Database already exists")
            }
            CoreError::DatabaseLocked => ("database_locked", "Database is locked"),
            CoreError::InvalidCredentials => {
                ("invalid_credentials", "Database credentials are invalid")
            }
            CoreError::InvalidSourceCredentials => (
                "source_invalid_credentials",
                "Source database credentials are invalid",
            ),
            CoreError::PasswordRequired => ("password_required", "Password is required"),
            CoreError::GroupNotFound => ("group_not_found", "Group does not exist"),
            CoreError::EntryNotFound => ("entry_not_found", "Entry does not exist"),
            CoreError::AttachmentNotFound => {
                ("attachment_not_found", "Entry attachment does not exist")
            }
            CoreError::InvalidNodeId => ("invalid_node_id", "Database node identifier is invalid"),
            CoreError::InvalidGroupMove => (
                "invalid_group_move",
                "Group cannot be moved to the requested location",
            ),
            CoreError::InvalidGroupDelete => ("invalid_group_delete", "Group cannot be deleted"),
            CoreError::InvalidEntryMove => (
                "invalid_entry_move",
                "Entry cannot be moved to the requested location",
            ),
            CoreError::InvalidGroupName => ("invalid_group_name", "Group name cannot be empty"),
            CoreError::InvalidEntryField => (
                "invalid_entry_field",
                "Entry field is invalid or is not protected",
            ),
            CoreError::InvalidEntryUpdate => ("invalid_entry_update", "Entry update is invalid"),
            CoreError::InvalidEntryDelete => (
                "invalid_entry_delete",
                "Entry cannot be deleted in the requested mode",
            ),
            CoreError::InvalidIconReference => {
                ("invalid_icon_reference", "Icon reference is invalid")
            }
            CoreError::InvalidTagName => ("invalid_tag_name", "Tag name cannot be empty"),
            CoreError::InvalidTagStyle => ("invalid_tag_style", "Tag style is invalid"),
            CoreError::MalformedTagStyles => {
                ("malformed_tag_styles", "Persisted tag styles are malformed")
            }
            CoreError::TagStyleNotFound => ("tag_style_not_found", "Tag style does not exist"),
            CoreError::TagInUse => ("tag_in_use", "Tag is used by an entry"),
            CoreError::InvalidPasskeyRequest(_) => {
                ("invalid_passkey_request", "Passkey request is invalid")
            }
            CoreError::PasskeyUnsupportedAlgorithm => (
                "passkey_unsupported_algorithm",
                "Passkey algorithm is not supported",
            ),
            CoreError::PasskeyExcluded => {
                ("passkey_excluded", "An excluded passkey already exists")
            }
            CoreError::PasskeyNotFound => ("passkey_not_found", "Passkey does not exist"),
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
