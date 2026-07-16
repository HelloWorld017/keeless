//! KDF parameters (backed by VariantDictionary)
//!

use uuid::Uuid;

use crate::kdbx::variant_dictionary::VariantDictionary;

/// KDF parameters stored as a VariantDictionary.
#[derive(Debug, Clone)]
pub struct KdfParameters {
    /// UUID of the KDF engine
    pub kdf_uuid: Uuid,
    /// Parameter storage
    pub dict: VariantDictionary,
}

impl KdfParameters {
    pub fn new(kdf_uuid: Uuid) -> Self {
        Self {
            kdf_uuid,
            dict: VariantDictionary::new(),
        }
    }

    /// Set the UUID parameter in the dictionary.
    pub fn set_uuid_param(&mut self) {
        let uuid_bytes = *self.kdf_uuid.as_bytes();
        self.dict.set_byte_array("$UUID", &uuid_bytes);
    }

    /// Get a byte array parameter
    pub fn get_byte_array(&self, name: &str) -> Option<&[u8]> {
        self.dict.get_byte_array(name)
    }

    /// Set a byte array parameter
    pub fn set_byte_array(&mut self, name: &str, value: &[u8]) {
        self.dict.set_byte_array(name, value);
    }

    /// Get a UInt64 parameter
    pub fn get_uint64(&self, name: &str) -> Option<u64> {
        self.dict.get_uint64(name)
    }

    /// Set a UInt64 parameter
    pub fn set_uint64(&mut self, name: &str, value: u64) {
        self.dict.set_uint64(name, value);
    }

    /// Get a UInt32 parameter
    pub fn get_uint32(&self, name: &str) -> Option<u32> {
        self.dict.get_uint32(name)
    }

    /// Set a UInt32 parameter
    pub fn set_uint32(&mut self, name: &str, value: u32) {
        self.dict.set_uint32(name, value);
    }

    /// Serialize to bytes
    pub fn serialize(&self) -> Vec<u8> {
        self.dict.serialize()
    }

    /// Deserialize from bytes
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        let dict = VariantDictionary::deserialize(data).ok()?;
        let uuid_bytes = dict.get_byte_array("$UUID")?;
        if uuid_bytes.len() != 16 {
            return None;
        }
        let kdf_uuid = Uuid::from_slice(uuid_bytes).ok()?;
        Some(Self { kdf_uuid, dict })
    }
}
