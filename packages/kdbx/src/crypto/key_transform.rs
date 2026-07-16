//! AES key transformation (AES-KDF)
//!
//! Transforms the master key using AES-256-ECB for the specified number of rounds,
//! then hashes with SHA-256.

use super::aes_cipher::AesCipher;
use super::hash::HashEngine;
use super::{CipherMode, CryptoResult};

/// AES-KDF key transformer.
/// Applies AES-256-ECB encryption to the key for `rounds` iterations, then SHA-256 hashes the result.
pub struct AesKeyTransformer;

impl AesKeyTransformer {
    /// Transform a key using AES-KDF.
    ///
    /// This is the core of the AES key derivation function used in KeePass:
    /// 1. Encrypt `key` with `seed` using AES-256-ECB, `rounds` times
    /// 2. SHA-256 hash the result
    ///
    /// # Arguments
    /// * `seed` - 32-byte seed (transformation key)
    /// * `key` - 32-byte key to transform
    /// * `rounds` - Number of transformation rounds
    ///
    pub fn transform_key(seed: &[u8], key: &[u8], rounds: u64) -> CryptoResult<Vec<u8>> {
        let cipher = AesCipher::new_ecb(CipherMode::Encrypt, seed)?;

        let mut current_key = key.to_vec();

        for _ in 0..rounds {
            current_key = cipher.process(&current_key)?;
        }

        // Final SHA-256 hash
        let hash = HashEngine::sha256(&current_key);
        Ok(hash.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_key_transform_basic() {
        let seed = [0x42u8; 32];
        let key = [0x13u8; 32];
        let rounds = 10;

        let result = AesKeyTransformer::transform_key(&seed, &key, rounds);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 32);
    }

    #[test]
    fn test_aes_key_transform_deterministic() {
        let seed = [0xABu8; 32];
        let key = [0xCDu8; 32];

        let result1 = AesKeyTransformer::transform_key(&seed, &key, 100).unwrap();
        let result2 = AesKeyTransformer::transform_key(&seed, &key, 100).unwrap();
        assert_eq!(result1, result2);
    }

    #[test]
    fn test_aes_key_transform_different_rounds_different_output() {
        let seed = [0x42u8; 32];
        let key = [0x13u8; 32];

        let r1 = AesKeyTransformer::transform_key(&seed, &key, 10).unwrap();
        let r2 = AesKeyTransformer::transform_key(&seed, &key, 100).unwrap();
        assert_ne!(r1, r2);
    }
}
