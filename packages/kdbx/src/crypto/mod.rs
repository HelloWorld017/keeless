//! Crypto primitives and abstractions for KeePass database layer.

// ─── Ciphers ────────────────────────────────────────────────────────
pub mod aes_cipher;
pub mod chacha20_cipher;
pub mod salsa20_cipher;
pub mod twofish_cipher;

// ─── Stream / Block ─────────────────────────────────────────────────
pub mod cipher_engine;
pub mod inner_stream;
pub mod stream_cipher;

// ─── Hash / HMAC ────────────────────────────────────────────────────
pub mod hash;
pub mod hmac_compute;

// ─── KDF / Key Transform ────────────────────────────────────────────
pub mod argon2_kdf;
pub mod key_transform;

// ─── Compression / Encryption ───────────────────────────────────────
pub mod compression;
pub mod encryption_algorithm;

// ─── Re-exports ─────────────────────────────────────────────────────
pub use cipher_engine::{CipherEngine, AesCipherEngine, ChaCha20CipherEngine, TwofishCipherEngine};
pub use compression::CompressionAlgorithm;
pub use encryption_algorithm::EncryptionAlgorithm;
pub use inner_stream::{InnerStreamCipher, Salsa20InnerStream, ChaCha20InnerStream, ArcFourInnerStream};
pub use aes_cipher::AesCipher;
pub use argon2_kdf::{Argon2Kdf, Argon2Params, Argon2Type};
pub use chacha20_cipher::ChaCha20Cipher;
pub use hash::HashEngine;
pub use hmac_compute::HmacCompute;
pub use key_transform::AesKeyTransformer;
pub use salsa20_cipher::Salsa20Cipher;
pub use stream_cipher::StreamCipher;
pub use twofish_cipher::TwofishCipher;

/// Cipher operation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CipherMode {
    Encrypt,
    Decrypt,
}

/// Block cipher mode of operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockMode {
    CBC,
    ECB,
    CTR,
}

/// Errors that can occur during cryptographic operations
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("Invalid key length: expected {expected}, got {got}")]
    InvalidKeyLength { expected: usize, got: usize },

    #[error("Invalid IV length: expected {expected}, got {got}")]
    InvalidIvLength { expected: usize, got: usize },

    #[error("Invalid data length: {0}")]
    InvalidDataLength(String),

    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),

    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),

    #[error("Argon2 error: {0}")]
    Argon2Error(String),

    #[error("Hash error: {0}")]
    HashError(String),

    #[error("HMAC error: {0}")]
    HmacError(String),
}

/// Result type for crypto operations
pub type CryptoResult<T> = Result<T, CryptoError>;
