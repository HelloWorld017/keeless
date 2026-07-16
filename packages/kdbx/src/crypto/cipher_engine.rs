//! Cipher engine abstraction
//!

use crate::crypto::{
    AesCipher, ChaCha20Cipher, CipherMode, CryptoResult, TwofishCipher,
};

use super::encryption_algorithm::EncryptionAlgorithm;

/// Abstract cipher engine for database encryption.
pub trait CipherEngine: Send + Sync {
    /// Key length in bytes
    fn key_length(&self) -> usize {
        32
    }

    /// IV length in bytes
    fn iv_length(&self) -> usize {
        16
    }

    /// Encrypt data
    fn encrypt(&self, key: &[u8], iv: &[u8], data: &[u8]) -> CryptoResult<Vec<u8>>;

    /// Decrypt data
    fn decrypt(&self, key: &[u8], iv: &[u8], data: &[u8]) -> CryptoResult<Vec<u8>>;

    /// Get the encryption algorithm
    fn algorithm(&self) -> EncryptionAlgorithm;
}

/// AES-256-CBC cipher engine.
pub struct AesCipherEngine;

impl CipherEngine for AesCipherEngine {
    fn encrypt(&self, key: &[u8], iv: &[u8], data: &[u8]) -> CryptoResult<Vec<u8>> {
        let cipher = AesCipher::new(CipherMode::Encrypt, key, iv)?;
        cipher.process(data)
    }

    fn decrypt(&self, key: &[u8], iv: &[u8], data: &[u8]) -> CryptoResult<Vec<u8>> {
        let cipher = AesCipher::new(CipherMode::Decrypt, key, iv)?;
        cipher.process(data)
    }

    fn algorithm(&self) -> EncryptionAlgorithm {
        EncryptionAlgorithm::AesRijndael
    }
}

/// Twofish-256-CBC cipher engine.
pub struct TwofishCipherEngine {
    force_compatibility: bool,
}

impl TwofishCipherEngine {
    pub fn new(force_compatibility: bool) -> Self {
        Self { force_compatibility }
    }
}

impl CipherEngine for TwofishCipherEngine {
    fn encrypt(&self, key: &[u8], iv: &[u8], data: &[u8]) -> CryptoResult<Vec<u8>> {
        let cipher = TwofishCipher::new(CipherMode::Encrypt, key, iv)?;
        if self.force_compatibility {
            cipher.without_padding().process(data)
        } else {
            cipher.process(data)
        }
    }

    fn decrypt(&self, key: &[u8], iv: &[u8], data: &[u8]) -> CryptoResult<Vec<u8>> {
        let cipher = TwofishCipher::new(CipherMode::Decrypt, key, iv)?;
        if self.force_compatibility {
            cipher.without_padding().process(data)
        } else {
            cipher.process(data)
        }
    }

    fn algorithm(&self) -> EncryptionAlgorithm {
        EncryptionAlgorithm::Twofish
    }
}

/// ChaCha20 cipher engine.
pub struct ChaCha20CipherEngine;

impl CipherEngine for ChaCha20CipherEngine {
    fn iv_length(&self) -> usize {
        12 // ChaCha20 uses 96-bit nonce
    }

    fn encrypt(&self, key: &[u8], iv: &[u8], data: &[u8]) -> CryptoResult<Vec<u8>> {
        let mut cipher = ChaCha20Cipher::new(key, iv)?;
        Ok(cipher.process(data))
    }

    fn decrypt(&self, key: &[u8], iv: &[u8], data: &[u8]) -> CryptoResult<Vec<u8>> {
        let mut cipher = ChaCha20Cipher::new(key, iv)?;
        Ok(cipher.process(data))
    }

    fn algorithm(&self) -> EncryptionAlgorithm {
        EncryptionAlgorithm::ChaCha20
    }
}

/// Create a cipher engine for the given encryption algorithm.
pub fn create_cipher_engine(algorithm: EncryptionAlgorithm) -> Box<dyn CipherEngine> {
    match algorithm {
        EncryptionAlgorithm::AesRijndael => Box::new(AesCipherEngine),
        EncryptionAlgorithm::Twofish => Box::new(TwofishCipherEngine::new(false)),
        EncryptionAlgorithm::ChaCha20 => Box::new(ChaCha20CipherEngine),
    }
}
