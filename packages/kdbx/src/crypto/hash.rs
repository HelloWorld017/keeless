//! SHA-256 / SHA-512 hash engine
//!
//! Provides SHA-256, SHA-512 hashing, and Salsa20/ChaCha20 stream cipher construction.

use sha2::{Digest, Sha256, Sha512};

/// Hashing engine providing SHA-256 and SHA-512.
pub struct HashEngine;

impl HashEngine {
    /// Compute SHA-256 hash of the input data.
    pub fn sha256(data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Compute SHA-256 hash of multiple byte slices (incremental update).
    pub fn sha256_multi(data_slices: &[&[u8]]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        for data in data_slices {
            hasher.update(data);
        }
        hasher.finalize().into()
    }

    /// Compute SHA-512 hash of the input data.
    pub fn sha512(data: &[u8]) -> [u8; 64] {
        let mut hasher = Sha512::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Compute SHA-512 hash of multiple byte slices.
    pub fn sha512_multi(data_slices: &[&[u8]]) -> [u8; 64] {
        let mut hasher = Sha512::new();
        for data in data_slices {
            hasher.update(data);
        }
        hasher.finalize().into()
    }

    /// Create a new SHA-256 hasher for incremental hashing.
    pub fn sha256_hasher() -> Sha256 {
        Sha256::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex;

    #[test]
    fn test_sha256_known_vector() {
        // NIST test vector
        let hash = HashEngine::sha256(b"abc");
        let expected =
            hex::decode("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
                .unwrap();
        assert_eq!(hash, expected.as_slice());
    }

    #[test]
    fn test_sha256_empty() {
        let hash = HashEngine::sha256(b"");
        let expected =
            hex::decode("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
                .unwrap();
        assert_eq!(hash, expected.as_slice());
    }

    #[test]
    fn test_sha512_known_vector() {
        let hash = HashEngine::sha512(b"abc");
        let expected_start = "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea2";
        let hex_str = hex::encode(hash);
        assert!(hex_str.starts_with(expected_start));
    }

    #[test]
    fn test_sha256_multi() {
        let single = HashEngine::sha256(b"hello world");
        let multi = HashEngine::sha256_multi(&[b"hello", b" world"]);
        assert_eq!(single, multi);
    }
}
