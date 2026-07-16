//! AES-KDF key derivation
//!

use crate::crypto::{AesKeyTransformer, HashEngine};
use uuid::Uuid;

use super::kdf_engine::KdfEngine;
use super::kdf_parameters::KdfParameters;
use crate::kdbx::limits::MAX_AES_KDF_ROUNDS;
use crate::model::exception::{DatabaseError, DatabaseResult};

/// AES-KDF UUID (standard KeePass)
pub const AES_KDF_UUID: Uuid = Uuid::from_bytes([
    0xC9, 0xD9, 0xF3, 0x9A, 0x62, 0x8A, 0x44, 0x60, 0xBF, 0x74, 0x0D, 0x08, 0xC1, 0x8A, 0x4F, 0xEA,
]);

const PARAM_ROUNDS: &str = "R"; // UInt64
const PARAM_SEED: &str = "S"; // Byte array

const DEFAULT_KEY_ROUNDS: u64 = 500_000;

/// AES-KDF engine.
/// Transforms the master key using AES-256-ECB for N rounds.
pub struct AesKdf;

impl KdfEngine for AesKdf {
    fn uuid(&self) -> Uuid {
        AES_KDF_UUID
    }

    fn transform(&self, master_key: &[u8], params: &KdfParameters) -> DatabaseResult<Vec<u8>> {
        let seed = params
            .get_byte_array(PARAM_SEED)
            .ok_or_else(|| DatabaseError::InvalidFormat("Missing AES-KDF seed".into()))?;
        if seed.len() != 32 {
            return Err(DatabaseError::InvalidFormat(
                "AES-KDF seed must be 32 bytes".into(),
            ));
        }

        let key_hashed;
        let key = if master_key.len() != 32 {
            key_hashed = HashEngine::sha256(master_key);
            key_hashed.as_slice()
        } else {
            master_key
        };

        let rounds = params
            .get_uint64(PARAM_ROUNDS)
            .ok_or_else(|| DatabaseError::InvalidFormat("Missing AES-KDF rounds".into()))?;
        if rounds == 0 || rounds > MAX_AES_KDF_ROUNDS {
            return Err(DatabaseError::InvalidFormat(
                "AES-KDF rounds are out of range".into(),
            ));
        }

        AesKeyTransformer::transform_key(seed, key, rounds)
            .map_err(|e| DatabaseError::DecryptionError(e.to_string()))
    }

    fn randomize(&self, params: &mut KdfParameters) -> DatabaseResult<()> {
        let mut seed = vec![0u8; 32];
        getrandom::getrandom(&mut seed).map_err(|e| {
            DatabaseError::EncryptionError(format!("secure random generation failed: {e}"))
        })?;
        params.set_byte_array(PARAM_SEED, &seed);
        Ok(())
    }

    fn default_parameters(&self) -> KdfParameters {
        let mut params = KdfParameters::new(self.uuid());
        params.set_uuid_param();
        params.set_uint64(PARAM_ROUNDS, DEFAULT_KEY_ROUNDS);
        params
    }

    fn get_key_rounds(&self, params: &KdfParameters) -> u64 {
        params
            .get_uint64(PARAM_ROUNDS)
            .unwrap_or(DEFAULT_KEY_ROUNDS)
    }

    fn set_key_rounds(&self, params: &mut KdfParameters, rounds: u64) {
        params.set_uint64(PARAM_ROUNDS, rounds);
    }

    fn default_key_rounds(&self) -> u64 {
        DEFAULT_KEY_ROUNDS
    }

    fn name(&self) -> &str {
        "AES"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_kdf_uuid() {
        let kdf = AesKdf;
        assert_eq!(kdf.uuid(), AES_KDF_UUID);
    }

    #[test]
    fn test_aes_kdf_transform() {
        let kdf = AesKdf;
        let mut params = kdf.default_parameters();
        params.set_byte_array(PARAM_SEED, &[0x42u8; 32]);

        let result = kdf.transform(&[0x13u8; 32], &params);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 32);
    }

    #[test]
    fn test_aes_kdf_deterministic() {
        let kdf = AesKdf;
        let mut params = kdf.default_parameters();
        params.set_uint64(PARAM_ROUNDS, 100);
        params.set_byte_array(PARAM_SEED, &[0xABu8; 32]);

        let r1 = kdf.transform(&[0xCDu8; 32], &params).unwrap();
        let r2 = kdf.transform(&[0xCDu8; 32], &params).unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_aes_kdf_rejects_missing_and_excessive_rounds() {
        let kdf = AesKdf;
        assert!(matches!(
            kdf.transform(&[0; 32], &KdfParameters::new(AES_KDF_UUID)),
            Err(DatabaseError::InvalidFormat(_))
        ));
        let mut params = kdf.default_parameters();
        params.set_byte_array(PARAM_SEED, &[0x42; 32]);
        params.set_uint64(PARAM_ROUNDS, MAX_AES_KDF_ROUNDS + 1);
        assert!(matches!(
            kdf.transform(&[0; 32], &params),
            Err(DatabaseError::InvalidFormat(_))
        ));
    }
}
