//! Encryption algorithm enum with KeePass UUID mappings
//!

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// KeePass encryption algorithm identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum EncryptionAlgorithm {
    /// AES-256 (Rijndael) CBC
    #[default]
    AesRijndael,
    /// Twofish-256 CBC
    Twofish,
    /// ChaCha20 stream cipher
    ChaCha20,
}

impl EncryptionAlgorithm {
    /// Get the KeePass UUID for this algorithm.
    /// These UUIDs are standardized in the KeePass file format.
    pub fn uuid(&self) -> Uuid {
        match self {
            EncryptionAlgorithm::AesRijndael => Uuid::from_bytes([
                0x31, 0xC1, 0xF2, 0xE6, 0xBF, 0x71, 0x43, 0x50,
                0xBE, 0x58, 0x05, 0x21, 0x6A, 0xFC, 0x5A, 0xFF,
            ]),
            EncryptionAlgorithm::Twofish => Uuid::from_bytes([
                0xAD, 0x68, 0xF2, 0x9F, 0x57, 0x6F, 0x4B, 0xB9,
                0xA3, 0x6A, 0xD4, 0x7A, 0xF9, 0x65, 0x34, 0x6C,
            ]),
            EncryptionAlgorithm::ChaCha20 => Uuid::from_bytes([
                0xD6, 0x03, 0x8A, 0x2B, 0x8B, 0x6F, 0x4C, 0xB5,
                0xA5, 0x24, 0x33, 0x9A, 0x31, 0xDB, 0xB5, 0x9A,
            ]),
        }
    }

    /// Get the encryption algorithm from a KeePass UUID.
    pub fn from_uuid(uuid: &Uuid) -> Option<Self> {
        match *uuid {
            u if u == EncryptionAlgorithm::AesRijndael.uuid() => Some(EncryptionAlgorithm::AesRijndael),
            u if u == EncryptionAlgorithm::Twofish.uuid() => Some(EncryptionAlgorithm::Twofish),
            u if u == EncryptionAlgorithm::ChaCha20.uuid() => Some(EncryptionAlgorithm::ChaCha20),
            _ => None,
        }
    }

    /// Key length in bytes (all algorithms use 256-bit keys)
    pub fn key_length(&self) -> usize {
        32
    }

    /// IV length in bytes
    pub fn iv_length(&self) -> usize {
        match self {
            EncryptionAlgorithm::AesRijndael => 16,
            EncryptionAlgorithm::Twofish => 16,
            EncryptionAlgorithm::ChaCha20 => 12,
        }
    }

    /// Human-readable name
    pub fn name(&self) -> &'static str {
        match self {
            EncryptionAlgorithm::AesRijndael => "Rijndael (AES)",
            EncryptionAlgorithm::Twofish => "Twofish",
            EncryptionAlgorithm::ChaCha20 => "ChaCha20",
        }
    }
}

impl std::fmt::Display for EncryptionAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_roundtrip() {
        for algo in [
            EncryptionAlgorithm::AesRijndael,
            EncryptionAlgorithm::Twofish,
            EncryptionAlgorithm::ChaCha20,
        ] {
            let uuid = algo.uuid();
            let recovered = EncryptionAlgorithm::from_uuid(&uuid);
            assert_eq!(recovered, Some(algo));
        }
    }

    #[test]
    fn test_unknown_uuid() {
        let unknown = Uuid::new_v4();
        assert_eq!(EncryptionAlgorithm::from_uuid(&unknown), None);
    }
}
