//! Argon2-KDF key derivation
//!

use crate::crypto::{argon2_kdf::Argon2Params, Argon2Kdf as Argon2KdfCore, Argon2Type};
use uuid::Uuid;

use super::kdf_engine::KdfEngine;
use super::kdf_parameters::KdfParameters;
use crate::kdbx::limits::{MAX_ARGON2_ITERATIONS, MAX_ARGON2_MEMORY_BYTES, MAX_ARGON2_PARALLELISM};
use crate::model::exception::{DatabaseError, DatabaseResult};

/// Argon2d KDF UUID
pub const ARGON2D_UUID: Uuid = Uuid::from_bytes([
    0xEF, 0x63, 0x6D, 0xDF, 0x8C, 0x29, 0x44, 0x4B, 0x91, 0xF7, 0xA9, 0xA4, 0x03, 0xE3, 0x0A, 0x0C,
]);

/// Argon2id KDF UUID
pub const ARGON2ID_UUID: Uuid = Uuid::from_bytes([
    0x9E, 0x29, 0x8B, 0x19, 0x56, 0xDB, 0x47, 0x73, 0xB2, 0x3D, 0xFC, 0x3E, 0xC6, 0xF0, 0xA1, 0xE6,
]);

const PARAM_SALT: &str = "S"; // byte[]
const PARAM_PARALLELISM: &str = "P"; // UInt32
const PARAM_MEMORY: &str = "M"; // UInt64
const PARAM_ITERATIONS: &str = "I"; // UInt64
const PARAM_VERSION: &str = "V"; // UInt32

const DEFAULT_ITERATIONS: u64 = 3;
const DEFAULT_MEMORY: u64 = 16 * 1024 * 1024; // 16 MB in KiB = 16384 KiB
const DEFAULT_PARALLELISM: u32 = 4;
const MAX_VERSION: u32 = 0x13;
const MEMORY_BLOCK_SIZE: u64 = 1024; // KiB per block

/// Argon2 KDF engine variant
#[derive(Debug, Clone, Copy)]
pub enum Argon2Variant {
    D,
    ID,
}

/// Argon2 KDF engine.
pub struct Argon2Kdf {
    variant: Argon2Variant,
}

impl Argon2Kdf {
    pub fn new(variant: Argon2Variant) -> Self {
        Self { variant }
    }

    pub fn argon2d() -> Self {
        Self::new(Argon2Variant::D)
    }

    pub fn argon2id() -> Self {
        Self::new(Argon2Variant::ID)
    }
}

impl KdfEngine for Argon2Kdf {
    fn uuid(&self) -> Uuid {
        match self.variant {
            Argon2Variant::D => ARGON2D_UUID,
            Argon2Variant::ID => ARGON2ID_UUID,
        }
    }

    fn transform(&self, master_key: &[u8], params: &KdfParameters) -> DatabaseResult<Vec<u8>> {
        let salt = params
            .get_byte_array(PARAM_SALT)
            .ok_or_else(|| DatabaseError::InvalidFormat("Missing Argon2 salt".into()))?;
        if salt.len() < 8 || salt.len() > 1024 {
            return Err(DatabaseError::InvalidFormat(
                "Invalid Argon2 salt length".into(),
            ));
        }
        let parallelism = params
            .get_uint32(PARAM_PARALLELISM)
            .ok_or_else(|| DatabaseError::InvalidFormat("Missing Argon2 parallelism".into()))?;
        let memory_bytes = params
            .get_uint64(PARAM_MEMORY)
            .ok_or_else(|| DatabaseError::InvalidFormat("Missing Argon2 memory".into()))?;
        let iterations = params
            .get_uint64(PARAM_ITERATIONS)
            .ok_or_else(|| DatabaseError::InvalidFormat("Missing Argon2 iterations".into()))?;
        let version = params
            .get_uint32(PARAM_VERSION)
            .ok_or_else(|| DatabaseError::InvalidFormat("Missing Argon2 version".into()))?;
        if parallelism == 0 || parallelism > MAX_ARGON2_PARALLELISM {
            return Err(DatabaseError::InvalidFormat(
                "Argon2 parallelism is out of range".into(),
            ));
        }
        if memory_bytes == 0
            || memory_bytes > MAX_ARGON2_MEMORY_BYTES
            || memory_bytes % MEMORY_BLOCK_SIZE != 0
        {
            return Err(DatabaseError::InvalidFormat(
                "Argon2 memory is out of range or not KiB-aligned".into(),
            ));
        }
        if memory_bytes / MEMORY_BLOCK_SIZE < u64::from(parallelism) * 8 {
            return Err(DatabaseError::InvalidFormat(
                "Argon2 memory is too small for its parallelism".into(),
            ));
        }
        if iterations == 0 || iterations > MAX_ARGON2_ITERATIONS {
            return Err(DatabaseError::InvalidFormat(
                "Argon2 iterations are out of range".into(),
            ));
        }
        if version != 0x10 && version != MAX_VERSION {
            return Err(DatabaseError::InvalidFormat(
                "Unsupported Argon2 version".into(),
            ));
        }
        let memory_kib = u32::try_from(memory_bytes / MEMORY_BLOCK_SIZE)
            .map_err(|_| DatabaseError::InvalidFormat("Argon2 memory is too large".into()))?;
        let iterations = u32::try_from(iterations)
            .map_err(|_| DatabaseError::InvalidFormat("Argon2 iterations are too large".into()))?;

        let argon_type = match self.variant {
            Argon2Variant::D => Argon2Type::D,
            Argon2Variant::ID => Argon2Type::ID,
        };

        let argon_params =
            Argon2Params::new(salt.to_vec(), parallelism, memory_kib, iterations, version);

        Argon2KdfCore::derive_key(argon_type, master_key, &argon_params)
            .map_err(|e| DatabaseError::DecryptionError(e.to_string()))
    }

    fn randomize(&self, params: &mut KdfParameters) -> DatabaseResult<()> {
        let mut salt = vec![0u8; 32];
        getrandom::getrandom(&mut salt).map_err(|e| {
            DatabaseError::EncryptionError(format!("secure random generation failed: {e}"))
        })?;
        params.set_byte_array(PARAM_SALT, &salt);
        Ok(())
    }

    fn default_parameters(&self) -> KdfParameters {
        let mut params = KdfParameters::new(self.uuid());
        params.set_uuid_param();
        params.set_uint32(PARAM_PARALLELISM, DEFAULT_PARALLELISM);
        params.set_uint64(PARAM_MEMORY, DEFAULT_MEMORY);
        params.set_uint64(PARAM_ITERATIONS, DEFAULT_ITERATIONS);
        params.set_uint32(PARAM_VERSION, MAX_VERSION);
        params
    }

    fn get_key_rounds(&self, params: &KdfParameters) -> u64 {
        params
            .get_uint64(PARAM_ITERATIONS)
            .unwrap_or(DEFAULT_ITERATIONS)
    }

    fn set_key_rounds(&self, params: &mut KdfParameters, rounds: u64) {
        params.set_uint64(PARAM_ITERATIONS, rounds);
    }

    fn default_key_rounds(&self) -> u64 {
        DEFAULT_ITERATIONS
    }

    fn name(&self) -> &str {
        match self.variant {
            Argon2Variant::D => "Argon2d",
            Argon2Variant::ID => "Argon2id",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_argon2id_transform() {
        let kdf = Argon2Kdf::argon2id();
        let mut params = kdf.default_parameters();
        params.set_byte_array(PARAM_SALT, &[0x42u8; 32]);
        params.set_uint64(PARAM_MEMORY, 1024 * 1024); // 1 MB

        let result = kdf.transform(b"testpassword", &params);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 32);
    }

    #[test]
    fn test_argon2_rejects_missing_and_excessive_parameters() {
        let kdf = Argon2Kdf::argon2id();
        assert!(matches!(
            kdf.transform(b"key", &KdfParameters::new(ARGON2ID_UUID)),
            Err(DatabaseError::InvalidFormat(_))
        ));

        let mut params = kdf.default_parameters();
        params.set_byte_array(PARAM_SALT, &[0x42; 32]);
        params.set_uint64(PARAM_MEMORY, MAX_ARGON2_MEMORY_BYTES + 1024);
        assert!(matches!(
            kdf.transform(b"key", &params),
            Err(DatabaseError::InvalidFormat(_))
        ));
    }
}
