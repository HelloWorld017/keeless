//! Composite key - combination of password, keyfile, and hardware key
//!

use keeless_secure_types::{SecureArray, SecureBytes};
use zeroize::Zeroizing;

use crate::crypto::HashEngine;
use crate::kdbx::kdf::{KdfEngine, KdfParameters};
use crate::model::exception::{DatabaseError, DatabaseResult};

/// A composite key combining multiple credential sources.
pub struct CompositeKey {
    password_data: Option<SecureBytes>,
    key_file_data: Option<SecureBytes>,
    hardware_key: Option<SecureBytes>,
    raw_key: Option<SecureArray<32>>,
}

impl CompositeKey {
    pub fn new() -> Self {
        Self {
            password_data: None,
            key_file_data: None,
            hardware_key: None,
            raw_key: None,
        }
    }

    /// Restore a composite key from its already-derived raw value.
    pub fn from_raw_key(raw_key: SecureArray<32>) -> Self {
        Self {
            password_data: None,
            key_file_data: None,
            hardware_key: None,
            raw_key: Some(raw_key),
        }
    }

    pub fn with_password(mut self, password: &[u8]) -> DatabaseResult<Self> {
        if self.raw_key.is_some() {
            return Err(DatabaseError::InvalidKey);
        }
        self.password_data = Some(SecureBytes::from_slice(password)?);
        Ok(self)
    }

    pub fn with_key_file(mut self, key_file_data: &[u8]) -> DatabaseResult<Self> {
        if self.raw_key.is_some() {
            return Err(DatabaseError::InvalidKey);
        }
        self.key_file_data = Some(SecureBytes::from_slice(key_file_data)?);
        Ok(self)
    }

    pub fn with_hardware_key(mut self, key: &[u8]) -> DatabaseResult<Self> {
        if self.raw_key.is_some() {
            return Err(DatabaseError::InvalidKey);
        }
        self.hardware_key = Some(SecureBytes::from_slice(key)?);
        Ok(self)
    }

    pub fn has_password(&self) -> bool {
        self.password_data.is_some()
    }

    pub fn has_key_file(&self) -> bool {
        self.key_file_data.is_some()
    }

    /// Build the raw composite key by hashing each component and combining.
    /// Returns the combined key bytes before KDF transformation.
    pub fn build_raw_key(&self) -> DatabaseResult<SecureArray<32>> {
        if let Some(raw_key) = &self.raw_key {
            return Ok(raw_key.try_clone()?);
        }

        let mut combined = Zeroizing::new(Vec::with_capacity(96));

        // Password component: SHA-256 hash
        if let Some(ref pwd) = self.password_data {
            let hash = Zeroizing::new(pwd.unlock_slice(HashEngine::sha256)?);
            combined.extend_from_slice(hash.as_slice());
        }

        // Key file component: SHA-256 hash (or raw if already 32 bytes)
        if let Some(ref kf) = self.key_file_data {
            if kf.len() == 32 {
                kf.unlock_slice(|value| combined.extend_from_slice(value))?;
            } else {
                let hash = Zeroizing::new(kf.unlock_slice(HashEngine::sha256)?);
                combined.extend_from_slice(hash.as_slice());
            }
        }

        // Hardware key component
        if let Some(ref hk) = self.hardware_key {
            hk.unlock_slice(|value| combined.extend_from_slice(value))?;
        }

        if combined.is_empty() {
            return Err(DatabaseError::InvalidKey);
        }

        // KeePass always hashes the concatenated user-key components,
        // including the common password-only case.
        let mut raw_key = HashEngine::sha256(&combined);
        Ok(SecureArray::from_array_mut(&mut raw_key)?)
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

/// Master credential wrapper.
#[derive(Debug)]
pub struct MasterCredential {
    pub composite_key: CompositeKey,
}

impl MasterCredential {
    pub fn new(composite_key: CompositeKey) -> Self {
        Self { composite_key }
    }

    pub fn from_password(password: &[u8]) -> DatabaseResult<Self> {
        Ok(Self {
            composite_key: CompositeKey::new().with_password(password)?,
        })
    }

    pub fn build_raw_key(&self) -> DatabaseResult<SecureArray<32>> {
        self.composite_key.build_raw_key()
    }
}

/// MakeFinalKey - derive the final encryption key from composite key + database headers.
/// This is the key derivation pipeline: rawKey → KDF transform → hash with masterSeed
pub fn make_final_key(
    composite_key: &CompositeKey,
    master_seed: &[u8],
    kdf_engine: &dyn KdfEngine,
    kdf_params: &KdfParameters,
) -> DatabaseResult<SecureArray<32>> {
    let raw_key = composite_key.build_raw_key()?;

    // KDF transform
    let transformed_key =
        SecureBytes::from_vec(raw_key.unlock(|value| kdf_engine.transform(value, kdf_params))??)?;

    // Final key = SHA-256(masterSeed || transformedKey)
    let mut combined = Zeroizing::new(Vec::with_capacity(
        master_seed.len() + transformed_key.len(),
    ));
    combined.extend_from_slice(master_seed);
    transformed_key.unlock_slice(|value| combined.extend_from_slice(value))?;

    let mut final_key = HashEngine::sha256(&combined);
    Ok(SecureArray::from_array_mut(&mut final_key)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{open_database, save_database, Database, DatabaseVersion, Group, NodeId};

    #[test]
    fn test_composite_key_password_only() {
        let key = CompositeKey::new().with_password(b"test123").unwrap();
        let raw = key.build_raw_key().unwrap();
        assert!(raw.unlock(|value| value.len() == 32).unwrap());
    }

    #[test]
    fn test_composite_key_multiple_components() {
        let key = CompositeKey::new()
            .with_password(b"test123")
            .unwrap()
            .with_key_file(b"keyfile_data");
        let raw = key.unwrap().build_raw_key().unwrap();
        assert!(raw.unlock(|value| value.len() == 32).unwrap());
    }

    #[test]
    fn test_composite_key_deterministic() {
        let key1 = CompositeKey::new().with_password(b"test").unwrap();
        let key2 = CompositeKey::new().with_password(b"test").unwrap();
        let raw1 = key1.build_raw_key().unwrap();
        let raw2 = key2.build_raw_key().unwrap();
        assert!(raw1
            .unlock(|left| raw2.unlock(|right| left == right))
            .unwrap()
            .unwrap());
    }

    #[test]
    fn test_composite_key_restores_independent_raw_key_clones() {
        let expected = [7; 32];
        let mut value = expected;
        let key = CompositeKey::from_raw_key(SecureArray::from_array_mut(&mut value).unwrap());

        let mut first = key.build_raw_key().unwrap();
        first.unlock_mut(|value| value.fill(0)).unwrap();
        let second = key.build_raw_key().unwrap();

        assert!(second.unlock(|value| value == &expected).unwrap());
    }

    #[test]
    fn test_raw_composite_key_rejects_components() {
        fn raw_key() -> SecureArray<32> {
            SecureArray::from_slice(&[7; 32]).unwrap()
        }

        assert!(matches!(
            CompositeKey::from_raw_key(raw_key()).with_password(b"password"),
            Err(DatabaseError::InvalidKey)
        ));
        assert!(matches!(
            CompositeKey::from_raw_key(raw_key()).with_key_file(b"key file"),
            Err(DatabaseError::InvalidKey)
        ));
        assert!(matches!(
            CompositeKey::from_raw_key(raw_key()).with_hardware_key(b"hardware key"),
            Err(DatabaseError::InvalidKey)
        ));
    }

    #[test]
    fn test_raw_composite_key_opens_password_database_without_rehashing() {
        let password_key = CompositeKey::new().with_password(b"password").unwrap();
        let restored_key = CompositeKey::from_raw_key(password_key.build_raw_key().unwrap());
        let mut database = Database::new(DatabaseVersion::KDBX4);
        let root_id = NodeId::new_uuid();
        database.groups.insert(root_id, Group::new(root_id));
        database.root_group_id = Some(root_id);
        let mut bytes = Vec::new();

        save_database(&mut bytes, &database, &password_key).unwrap();
        open_database(bytes.as_slice(), &restored_key).unwrap();
    }
}
