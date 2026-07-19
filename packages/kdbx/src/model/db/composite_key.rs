//! Composite key - combination of password, keyfile, and hardware key
//!

use base64::Engine;
use keeless_secure_types::{SecureArray, SecureBytes};
use quick_xml::events::Event;
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

    /// Add a KeePass key file, decoding standard XML and hex key-file formats.
    ///
    /// Arbitrary binary files retain KeePass's normal SHA-256 fallback through
    /// [`Self::with_key_file`].
    pub fn with_key_file_contents(self, contents: &[u8]) -> DatabaseResult<Self> {
        if contents.len() == 32 {
            return self.with_key_file(contents);
        }

        if let Some(decoded) = decode_hex_key(contents) {
            return self.with_key_file(&decoded);
        }

        if contents
            .iter()
            .copied()
            .find(|byte| !byte.is_ascii_whitespace())
            == Some(b'<')
        {
            let decoded = Zeroizing::new(decode_xml_key_file(contents)?);
            return self.with_key_file(decoded.as_slice());
        }

        self.with_key_file(contents)
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

fn decode_xml_key_file(contents: &[u8]) -> DatabaseResult<Vec<u8>> {
    let mut reader = quick_xml::Reader::from_reader(contents);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut in_data = false;
    let mut encoded = String::new();
    let mut expected_hash = None;

    loop {
        buffer.clear();
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(start)) if start.name().as_ref() == b"Data" => {
                if in_data || !encoded.is_empty() {
                    return Err(DatabaseError::InvalidFormat(
                        "KeePass key file contains duplicate Data elements".into(),
                    ));
                }
                in_data = true;
                for attribute in start.attributes() {
                    let attribute = attribute.map_err(|error| {
                        DatabaseError::InvalidFormat(format!(
                            "Invalid KeePass key-file attribute: {error}"
                        ))
                    })?;
                    if attribute.key.as_ref() == b"Hash" {
                        expected_hash = Some(
                            std::str::from_utf8(attribute.value.as_ref())
                                .map_err(|error| {
                                    DatabaseError::InvalidFormat(format!(
                                        "Invalid KeePass key-file hash: {error}"
                                    ))
                                })?
                                .to_string(),
                        );
                    }
                }
            }
            Ok(Event::Text(text)) if in_data => {
                encoded.push_str(&text.unescape().map_err(|error| {
                    DatabaseError::InvalidFormat(format!("Invalid KeePass key-file data: {error}"))
                })?);
            }
            Ok(Event::CData(data)) if in_data => {
                encoded.push_str(std::str::from_utf8(data.as_ref()).map_err(|error| {
                    DatabaseError::InvalidFormat(format!("Invalid KeePass key-file data: {error}"))
                })?);
            }
            Ok(Event::End(end)) if end.name().as_ref() == b"Data" => {
                in_data = false;
            }
            Ok(Event::DocType(_)) => {
                return Err(DatabaseError::InvalidFormat(
                    "DTD is not allowed in a KeePass key file".into(),
                ));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(DatabaseError::InvalidFormat(format!(
                    "Malformed KeePass key file: {error}"
                )));
            }
        }
    }

    if in_data || encoded.is_empty() {
        return Err(DatabaseError::InvalidFormat(
            "KeePass key file has no complete Data element".into(),
        ));
    }

    let compact: String = encoded
        .chars()
        .filter(|char| !char.is_whitespace())
        .collect();
    let decoded = if expected_hash.is_some() {
        decode_hex(&compact).ok_or_else(|| {
            DatabaseError::InvalidFormat("KeePass XML v2 key data is not valid hex".into())
        })?
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(&compact)
            .map_err(|error| {
                DatabaseError::InvalidFormat(format!(
                    "KeePass XML v1 key data is not valid base64: {error}"
                ))
            })?
    };
    if decoded.len() != 32 {
        return Err(DatabaseError::InvalidFormat(format!(
            "KeePass key data must be 32 bytes, got {}",
            decoded.len()
        )));
    }

    if let Some(expected_hash) = expected_hash {
        let expected = decode_hex(expected_hash.trim()).ok_or_else(|| {
            DatabaseError::InvalidFormat("KeePass key-file Hash is not valid hex".into())
        })?;
        let actual = HashEngine::sha256(&decoded);
        if expected.as_slice() != &actual[..expected.len().min(actual.len())]
            || expected.is_empty()
            || expected.len() > actual.len()
        {
            return Err(DatabaseError::IntegrityError(
                "KeePass key-file hash mismatch".into(),
            ));
        }
    }

    Ok(decoded)
}

fn decode_hex_key(contents: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(contents).ok()?.trim();
    if text.len() != 64 {
        return None;
    }
    let decoded = decode_hex(text)?;
    (decoded.len() == 32).then_some(decoded)
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            Some(((high << 4) | low) as u8)
        })
        .collect()
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
    fn standard_key_file_formats_produce_the_same_component() {
        let key_bytes = [0x42; 32];
        let hex = "42".repeat(32);
        let xml_v1 = format!(
            "<KeyFile><Meta><Version>1.00</Version></Meta><Key><Data>{}</Data></Key></KeyFile>",
            base64::engine::general_purpose::STANDARD.encode(key_bytes)
        );
        let hash = HashEngine::sha256(&key_bytes);
        let hash_prefix: String = hash[..4].iter().map(|byte| format!("{byte:02X}")).collect();
        let xml_v2 = format!(
            "<KeyFile><Meta><Version>2.0</Version></Meta><Key><Data Hash=\"{hash_prefix}\">{hex}</Data></Key></KeyFile>"
        );

        let expected = CompositeKey::new()
            .with_key_file(&key_bytes)
            .unwrap()
            .build_raw_key()
            .unwrap();
        for contents in [hex.as_bytes(), xml_v1.as_bytes(), xml_v2.as_bytes()] {
            let actual = CompositeKey::new()
                .with_key_file_contents(contents)
                .unwrap()
                .build_raw_key()
                .unwrap();
            assert!(expected
                .unlock(|left| actual.unlock(|right| left == right))
                .unwrap()
                .unwrap());
        }
    }

    #[test]
    fn xml_v2_key_file_rejects_bad_hash() {
        let xml = format!(
            "<KeyFile><Key><Data Hash=\"00000000\">{}</Data></Key></KeyFile>",
            "42".repeat(32)
        );
        assert!(matches!(
            CompositeKey::new().with_key_file_contents(xml.as_bytes()),
            Err(DatabaseError::IntegrityError(_))
        ));
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
