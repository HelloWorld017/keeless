//! Error types for KeePass database operations

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid file signature: {0}")]
    InvalidSignature(String),

    #[error("Invalid file version: {0}")]
    InvalidVersion(String),

    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    #[error("Invalid key")]
    InvalidKey,

    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Encryption error: {0}")]
    EncryptionError(String),

    #[error("Decryption error: {0}")]
    DecryptionError(String),

    #[error("Compression error: {0}")]
    CompressionError(String),

    #[error("XML parse error: {0}")]
    XmlError(String),

    #[error("Database not loaded")]
    NotLoaded,

    #[error("Database already loaded")]
    AlreadyLoaded,

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Merge error: {0}")]
    MergeError(String),

    #[error("Search error: {0}")]
    SearchError(String),

    #[error("Integrity check failed: {0}")]
    IntegrityError(String),

    #[error("Unsupported feature: {0}")]
    Unsupported(String),

    #[error("Secure memory error: {0}")]
    SecureMemory(String),

    #[error("Crypto error: {0}")]
    CryptoError(#[from] crate::crypto::CryptoError),

    #[error("XML error: {0}")]
    QuickXmlError(String),
}

impl From<quick_xml::Error> for DatabaseError {
    fn from(e: quick_xml::Error) -> Self {
        DatabaseError::QuickXmlError(e.to_string())
    }
}

impl From<quick_xml::events::attributes::AttrError> for DatabaseError {
    fn from(e: quick_xml::events::attributes::AttrError) -> Self {
        DatabaseError::QuickXmlError(e.to_string())
    }
}

impl From<keeless_secure_types::Error> for DatabaseError {
    fn from(error: keeless_secure_types::Error) -> Self {
        Self::SecureMemory(error.to_string())
    }
}

impl DatabaseError {
    pub(crate) fn from_encryption_error(error: crate::crypto::CryptoError) -> Self {
        match error {
            crate::crypto::CryptoError::SecureMemory(message) => Self::SecureMemory(message),
            error => Self::EncryptionError(error.to_string()),
        }
    }

    pub(crate) fn from_decryption_error(error: crate::crypto::CryptoError) -> Self {
        match error {
            crate::crypto::CryptoError::SecureMemory(message) => Self::SecureMemory(message),
            error => Self::DecryptionError(error.to_string()),
        }
    }
}

pub type DatabaseResult<T> = Result<T, DatabaseError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_memory_errors_keep_their_classification() {
        let encryption = DatabaseError::from_encryption_error(
            crate::crypto::CryptoError::SecureMemory("lock failed".into()),
        );
        let decryption = DatabaseError::from_decryption_error(
            crate::crypto::CryptoError::SecureMemory("lock failed".into()),
        );

        assert!(matches!(encryption, DatabaseError::SecureMemory(_)));
        assert!(matches!(decryption, DatabaseError::SecureMemory(_)));
    }
}
