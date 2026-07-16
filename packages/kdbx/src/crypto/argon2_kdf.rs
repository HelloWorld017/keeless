//! Argon2 key derivation function
//!
//! Supports Argon2d, Argon2i, Argon2id variants.

use argon2::{Algorithm, Argon2, Params, Version};

use super::{CryptoError, CryptoResult};

/// Argon2 variant type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Argon2Type {
    /// Argon2d - Data-dependent memory access
    D,
    /// Argon2i - Data-independent memory access
    I,
    /// Argon2id - Hybrid approach (recommended)
    ID,
}

impl Argon2Type {
    fn to_algorithm(self) -> Algorithm {
        match self {
            Argon2Type::D => Algorithm::Argon2d,
            Argon2Type::I => Algorithm::Argon2i,
            Argon2Type::ID => Algorithm::Argon2id,
        }
    }
}

/// Argon2 KDF parameters
#[derive(Debug, Clone)]
pub struct Argon2Params {
    /// Salt
    pub salt: Vec<u8>,
    /// Number of lanes (parallelism)
    pub parallelism: u32,
    /// Memory cost in KiB
    pub memory_cost: u32,
    /// Number of iterations (time cost)
    pub iterations: u32,
    /// Argon2 version (0x10 or 0x13)
    pub version: u32,
    /// Output hash length (default: 32)
    pub hash_length: usize,
}

impl Argon2Params {
    pub fn new(salt: Vec<u8>, parallelism: u32, memory_cost: u32, iterations: u32, version: u32) -> Self {
        Self {
            salt,
            parallelism,
            memory_cost,
            iterations,
            version,
            hash_length: 32,
        }
    }
}

/// Argon2 key derivation
pub struct Argon2Kdf;

impl Argon2Kdf {
    /// Derive a key using Argon2.
    ///
    /// # Arguments
    /// * `argon2_type` - Argon2 variant (D, I, or ID)
    /// * `password` - Master key / password bytes
    /// * `params` - Argon2 parameters
    ///
    /// # Returns
    /// 32-byte derived key
    pub fn derive_key(
        argon2_type: Argon2Type,
        password: &[u8],
        params: &Argon2Params,
    ) -> CryptoResult<Vec<u8>> {
        let version = match params.version {
            0x10 => Version::V0x10,
            0x13 => Version::V0x13,
            _ => return Err(CryptoError::Argon2Error(format!(
                "Unsupported Argon2 version: {:#x}", params.version
            ))),
        };

        let algorithm = argon2_type.to_algorithm();

        let argon2 = Argon2::new(
            algorithm,
            version,
            Params::new(
                params.memory_cost,
                params.iterations,
                params.parallelism,
                Some(params.hash_length),
            )
            .map_err(|e| CryptoError::Argon2Error(e.to_string()))?,
        );

        let mut output = vec![0u8; params.hash_length];
        argon2
            .hash_password_into(password, &params.salt, &mut output)
            .map_err(|e| CryptoError::Argon2Error(e.to_string()))?;

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_argon2id_basic() {
        let salt = vec![0xABu8; 32];
        let params = Argon2Params::new(
            salt,
            4,     // parallelism
            1024,  // memory (1 MB)
            3,     // iterations
            0x13,  // version 1.3
        );

        let result = Argon2Kdf::derive_key(Argon2Type::ID, b"testpassword", &params);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 32);
    }

    #[test]
    fn test_argon2d_basic() {
        let salt = vec![0x42u8; 32];
        let params = Argon2Params::new(salt, 2, 1024, 2, 0x13);

        let result = Argon2Kdf::derive_key(Argon2Type::D, b"password123", &params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_argon2_deterministic() {
        let salt = vec![0x11u8; 32];
        let params = Argon2Params::new(salt, 1, 1024, 1, 0x13);

        let result1 = Argon2Kdf::derive_key(Argon2Type::ID, b"test", &params).unwrap();
        let result2 = Argon2Kdf::derive_key(Argon2Type::ID, b"test", &params).unwrap();
        assert_eq!(result1, result2);
    }
}
