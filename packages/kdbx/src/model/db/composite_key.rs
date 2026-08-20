//! Database credentials and transformed composite keys.
//!

use base64::Engine;
use keeless_secure_types::{SecureArray, SecureBytes};
use quick_xml::events::Event;
use zeroize::Zeroizing;

use crate::crypto::HashEngine;
use crate::kdbx::kdf::{create_kdf, KdfParameters};
use crate::model::exception::{DatabaseError, DatabaseResult};

/// Credential material used only while deriving a database key.
pub struct CompositeCredentials {
    password_data: Option<SecureBytes>,
    key_file_data: Option<SecureBytes>,
    hardware_key: Option<SecureBytes>,
}

impl CompositeCredentials {
    pub fn new() -> Self {
        Self {
            password_data: None,
            key_file_data: None,
            hardware_key: None,
        }
    }

    pub fn with_password(mut self, password: &[u8]) -> DatabaseResult<Self> {
        self.password_data = Some(SecureBytes::from_slice(password)?);
        Ok(self)
    }

    pub fn with_key_file(mut self, key_file_data: &[u8]) -> DatabaseResult<Self> {
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
    ///
    /// This remains crate-visible so only format readers and key derivation can
    /// access raw credential material.
    pub(crate) fn build_raw_key(&self) -> DatabaseResult<SecureArray<32>> {
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

    /// Derive the transformed key tied to a database's KDF parameters.
    pub fn derive_key(&self, parameters: &KdfParameters) -> DatabaseResult<CompositeKey> {
        let raw_key = self.build_raw_key()?;
        let kdf = create_kdf(&parameters.kdf_uuid)
            .ok_or_else(|| DatabaseError::InvalidFormat("Unknown KDF".into()))?;
        let mut transformed = raw_key.unlock(|key| kdf.transform(key, parameters))??;
        let transformed: &mut [u8; 32] = transformed
            .as_mut_slice()
            .try_into()
            .map_err(|_| DatabaseError::InvalidFormat("KDF output must be 32 bytes".into()))?;
        let key = SecureArray::from_array_mut(transformed)?;
        Ok(CompositeKey::new(key, parameters))
    }
}

/// A transformed composite key tied to one set of database KDF parameters.
pub struct CompositeKey {
    key: SecureArray<32>,
    kdf_fingerprint: [u8; 32],
}

impl CompositeKey {
    pub(crate) fn new(key: SecureArray<32>, parameters: &KdfParameters) -> Self {
        Self {
            key,
            kdf_fingerprint: kdf_fingerprint(parameters),
        }
    }

    /// Restore a transformed key from secure storage.
    pub fn from_derived_key(key: SecureArray<32>, kdf_fingerprint: [u8; 32]) -> Self {
        Self {
            key,
            kdf_fingerprint,
        }
    }

    pub fn matches(&self, parameters: &KdfParameters) -> bool {
        self.kdf_fingerprint == kdf_fingerprint(parameters)
    }

    pub fn kdf_fingerprint(&self) -> [u8; 32] {
        self.kdf_fingerprint
    }

    pub fn try_clone(&self) -> DatabaseResult<Self> {
        Ok(Self {
            key: self.key.try_clone()?,
            kdf_fingerprint: self.kdf_fingerprint,
        })
    }

    pub fn with_key<T>(&self, use_key: impl FnOnce(&[u8; 32]) -> T) -> DatabaseResult<T> {
        Ok(self.key.unlock(use_key)?)
    }

    /// Derive a domain-separated runtime key without repeating the database KDF.
    pub fn derive_key<const N: usize>(
        &self,
        salt: Option<&[u8]>,
        info: &[u8],
    ) -> DatabaseResult<SecureArray<N>> {
        use hkdf::Hkdf;
        use sha2::Sha256;

        let mut derived = SecureArray::zeroed()?;
        let result = self.key.unlock(|key| {
            derived.unlock_mut(|value| Hkdf::<Sha256>::new(salt, key).expand(info, value))
        })??;
        result.map_err(|_| DatabaseError::EncryptionError("key derivation failed".into()))?;
        Ok(derived)
    }
}

impl std::fmt::Debug for CompositeKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeKey")
            .field("kdf_fingerprint", &self.kdf_fingerprint)
            .finish_non_exhaustive()
    }
}

fn kdf_fingerprint(parameters: &KdfParameters) -> [u8; 32] {
    let mut dict = parameters.dict.clone();
    // KDBX 3.1 stores its KDF UUID outside the variant dictionary. Normalize
    // the optional KDBX4 $UUID entry so both representations fingerprint alike.
    dict.remove("$UUID");
    let serialized = dict.serialize();
    HashEngine::sha256_multi(&[parameters.kdf_uuid.as_bytes(), &serialized])
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

impl std::fmt::Debug for CompositeCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeCredentials")
            .field("has_password", &self.password_data.is_some())
            .field("has_key_file", &self.key_file_data.is_some())
            .field("has_hardware_key", &self.hardware_key.is_some())
            .finish()
    }
}

impl Default for CompositeCredentials {
    fn default() -> Self {
        Self::new()
    }
}

/// Master credential wrapper.
#[derive(Debug)]
pub struct MasterCredential {
    pub credentials: CompositeCredentials,
}

impl MasterCredential {
    pub fn new(credentials: CompositeCredentials) -> Self {
        Self { credentials }
    }

    pub fn from_password(password: &[u8]) -> DatabaseResult<Self> {
        Ok(Self {
            credentials: CompositeCredentials::new().with_password(password)?,
        })
    }

    pub fn derive_key(&self, parameters: &KdfParameters) -> DatabaseResult<CompositeKey> {
        self.credentials.derive_key(parameters)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kdbx::kdf::aes_kdf::AES_KDF_UUID;

    fn parameters(seed: u8) -> KdfParameters {
        let mut parameters = KdfParameters::new(AES_KDF_UUID);
        parameters.set_byte_array("S", &[seed; 32]);
        parameters.set_uint64("R", 1);
        parameters
    }

    #[test]
    fn same_credentials_and_parameters_derive_the_same_key() {
        let credentials = CompositeCredentials::new()
            .with_password(b"test123")
            .unwrap();
        let first = credentials.derive_key(&parameters(1)).unwrap();
        let second = credentials.derive_key(&parameters(1)).unwrap();
        assert_eq!(first.kdf_fingerprint(), second.kdf_fingerprint());
        assert!(first
            .with_key(|first| second.with_key(|second| first == second))
            .unwrap()
            .unwrap());
    }

    #[test]
    fn changed_kdf_parameters_change_the_key_and_fingerprint() {
        let credentials = CompositeCredentials::new()
            .with_password(b"test123")
            .unwrap();
        let first = credentials.derive_key(&parameters(1)).unwrap();
        let second = credentials.derive_key(&parameters(2)).unwrap();
        assert_ne!(first.kdf_fingerprint(), second.kdf_fingerprint());
        assert!(!first.matches(&parameters(2)));
    }

    #[test]
    fn different_credentials_derive_different_keys() {
        let first = CompositeCredentials::new()
            .with_password(b"first")
            .unwrap()
            .derive_key(&parameters(1))
            .unwrap();
        let second = CompositeCredentials::new()
            .with_password(b"second")
            .unwrap()
            .derive_key(&parameters(1))
            .unwrap();
        assert!(first
            .with_key(|first| second.with_key(|second| first != second))
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

        let expected = CompositeCredentials::new()
            .with_key_file(&key_bytes)
            .unwrap()
            .derive_key(&parameters(1))
            .unwrap();
        for contents in [hex.as_bytes(), xml_v1.as_bytes(), xml_v2.as_bytes()] {
            let actual = CompositeCredentials::new()
                .with_key_file_contents(contents)
                .unwrap()
                .derive_key(&parameters(1))
                .unwrap();
            assert!(expected
                .with_key(|left| actual.with_key(|right| left == right))
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
            CompositeCredentials::new().with_key_file_contents(xml.as_bytes()),
            Err(DatabaseError::IntegrityError(_))
        ));
    }

    #[test]
    fn derived_runtime_keys_are_domain_separated() {
        let key = CompositeCredentials::new()
            .with_password(b"password")
            .unwrap()
            .derive_key(&parameters(1))
            .unwrap();
        let first = key.derive_key::<32>(None, b"first").unwrap();
        let second = key.derive_key::<32>(None, b"second").unwrap();
        assert!(first
            .unlock(|first| second.unlock(|second| first != second))
            .unwrap()
            .unwrap());
    }
}
