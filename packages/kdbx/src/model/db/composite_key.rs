//! Composite key - combination of password, keyfile, and hardware key
//!

use zeroize::Zeroize;

use crate::crypto::HashEngine;
use crate::model::exception::{DatabaseError, DatabaseResult};

/// A composite key combining multiple credential sources.
pub struct CompositeKey {
    password_data: Option<Vec<u8>>,
    key_file_data: Option<Vec<u8>>,
    hardware_key: Option<Vec<u8>>,
}

impl CompositeKey {
    pub fn new() -> Self {
        Self {
            password_data: None,
            key_file_data: None,
            hardware_key: None,
        }
    }

    pub fn with_password(mut self, password: &[u8]) -> Self {
        self.password_data = Some(password.to_vec());
        self
    }

    pub fn with_key_file(mut self, key_file_data: &[u8]) -> Self {
        self.key_file_data = Some(key_file_data.to_vec());
        self
    }

    pub fn with_hardware_key(mut self, key: &[u8]) -> Self {
        self.hardware_key = Some(key.to_vec());
        self
    }

    pub fn has_password(&self) -> bool {
        self.password_data.is_some()
    }

    pub fn has_key_file(&self) -> bool {
        self.key_file_data.is_some()
    }

    /// Build the raw composite key by hashing each component and combining.
    /// Returns the combined key bytes before KDF transformation.
    pub fn build_raw_key(&self) -> Vec<u8> {
        let mut components: Vec<Vec<u8>> = Vec::new();

        // Password component: SHA-256 hash
        if let Some(ref pwd) = self.password_data {
            let hash = HashEngine::sha256(pwd);
            components.push(hash.to_vec());
        }

        // Key file component: SHA-256 hash (or raw if already 32 bytes)
        if let Some(ref kf) = self.key_file_data {
            if kf.len() == 32 {
                components.push(kf.clone());
            } else {
                let hash = HashEngine::sha256(kf);
                components.push(hash.to_vec());
            }
        }

        // Hardware key component
        if let Some(ref hk) = self.hardware_key {
            components.push(hk.clone());
        }

        // Combine all components
        if components.is_empty() {
            return Vec::new();
        }

        // KeePass always hashes the concatenated user-key components,
        // including the common password-only case.
        let mut combined = Vec::new();
        for c in &components {
            combined.extend_from_slice(c);
        }
        HashEngine::sha256(&combined).to_vec()
    }
}

impl std::fmt::Debug for CompositeKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeKey")
            .field("has_password", &self.password_data.is_some())
            .field("has_key_file", &self.key_file_data.is_some())
            .field("has_hardware_key", &self.hardware_key.is_some())
            .finish()
    }
}

impl Default for CompositeKey {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CompositeKey {
    fn drop(&mut self) {
        if let Some(ref mut d) = self.password_data {
            d.zeroize();
        }
        if let Some(ref mut d) = self.key_file_data {
            d.zeroize();
        }
        if let Some(ref mut d) = self.hardware_key {
            d.zeroize();
        }
    }
}

/// Master credential wrapper.
#[derive(Debug)]
pub struct MasterCredential {
    pub composite_key: CompositeKey,
}

impl MasterCredential {
    pub fn new(composite_key: CompositeKey) -> Self {
        Self { composite_key }
    }

    pub fn from_password(password: &[u8]) -> Self {
        Self {
            composite_key: CompositeKey::new().with_password(password),
        }
    }

    pub fn build_raw_key(&self) -> Vec<u8> {
        self.composite_key.build_raw_key()
    }
}

/// MakeFinalKey - derive the final encryption key from composite key + database headers.
/// This is the key derivation pipeline: rawKey → KDF transform → hash with masterSeed
pub fn make_final_key(
    composite_key: &CompositeKey,
    master_seed: &[u8],
    kdf_engine: &dyn crate::kdbx::kdf::KdfEngine,
    kdf_params: &crate::kdbx::kdf::KdfParameters,
) -> DatabaseResult<Vec<u8>> {
    let raw_key = composite_key.build_raw_key();
    if raw_key.is_empty() {
        return Err(DatabaseError::InvalidKey);
    }

    // KDF transform
    let transformed_key = kdf_engine.transform(&raw_key, kdf_params)?;

    // Final key = SHA-256(masterSeed || transformedKey)
    let mut combined = Vec::with_capacity(master_seed.len() + transformed_key.len());
    combined.extend_from_slice(master_seed);
    combined.extend_from_slice(&transformed_key);

    let final_key = HashEngine::sha256(&combined);
    Ok(final_key.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_composite_key_password_only() {
        let key = CompositeKey::new().with_password(b"test123");
        let raw = key.build_raw_key();
        assert_eq!(raw.len(), 32);
    }

    #[test]
    fn test_composite_key_multiple_components() {
        let key = CompositeKey::new()
            .with_password(b"test123")
            .with_key_file(b"keyfile_data");
        let raw = key.build_raw_key();
        assert_eq!(raw.len(), 32);
    }

    #[test]
    fn test_composite_key_deterministic() {
        let key1 = CompositeKey::new().with_password(b"test");
        let key2 = CompositeKey::new().with_password(b"test");
        assert_eq!(key1.build_raw_key(), key2.build_raw_key());
    }
}
