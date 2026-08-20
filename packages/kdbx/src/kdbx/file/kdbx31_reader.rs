//! KDBX 3.1 complete read pipeline
//!
//! Pipeline: signature → outer header → key derivation → decrypt → verify streamStartBytes
//!           → hashed block stream → decompress → inner stream decrypt → XML → Database

use base64::Engine;
use keeless_secure_types::SecureArray;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::{BufRead, Read};
use zeroize::Zeroizing;

use crate::crypto::cipher_engine::create_cipher_engine;
use crate::crypto::compression::{decompress_sensitive, CompressionAlgorithm};
use crate::crypto::inner_stream::create_inner_stream;
use crate::crypto::HashEngine;
use crate::kdbx::diagnostics::{DiagnosticContext, DiagnosticStage};
use crate::kdbx::file::header::KdbxHeader31;
use crate::kdbx::file::reader::{DatabaseReader, TeeReader};
use crate::kdbx::kdf::aes_kdf::AES_KDF_UUID;
use crate::kdbx::kdf::kdf_parameters::KdfParameters;
use crate::kdbx::stream::hashed_block::HashedBlockReader;
use crate::kdbx::xml::KdbxXmlReader;
use crate::model::db::composite_key::{CompositeCredentials, CompositeKey};
use crate::model::db::database::{Database, DatabaseVersion};
use crate::model::exception::{DatabaseError, DatabaseResult};

/// Read a KDBX 3.1 database from a reader.
pub fn read_kdbx31<R: Read>(
    reader: &mut R,
    composite_key: &CompositeKey,
) -> DatabaseResult<Database> {
    let mut diagnostics = DiagnosticContext::disabled();
    read_kdbx31_diagnostic(reader, composite_key, &mut diagnostics)
}

pub(crate) fn read_kdbx31_with_credentials_diagnostic<R: Read>(
    reader: &mut R,
    credentials: &CompositeCredentials,
    diagnostics: &mut DiagnosticContext<'_>,
) -> DatabaseResult<(Database, CompositeKey)> {
    read_kdbx31_with_key_deriver(reader, |params| credentials.derive_key(params), diagnostics)
}

pub(crate) fn read_kdbx31_diagnostic<R: Read>(
    reader: &mut R,
    composite_key: &CompositeKey,
    diagnostics: &mut DiagnosticContext<'_>,
) -> DatabaseResult<Database> {
    read_kdbx31_with_key_deriver(
        reader,
        |params| {
            if composite_key.matches(params) {
                composite_key.try_clone()
            } else {
                Err(DatabaseError::KdfParametersMismatch)
            }
        },
        diagnostics,
    )
    .map(|(database, _)| database)
}

fn read_kdbx31_with_key_deriver<R: Read>(
    reader: &mut R,
    derive_composite_key: impl FnOnce(&KdfParameters) -> DatabaseResult<CompositeKey>,
    diagnostics: &mut DiagnosticContext<'_>,
) -> DatabaseResult<(Database, CompositeKey)> {
    // 1. Read and verify signature while retaining the exact header bytes.
    let mut header_buf = Vec::new();
    let version = {
        let mut tee = TeeReader::new(reader, &mut header_buf);
        DatabaseReader::detect_version(&mut tee)?
    };
    if version != DatabaseVersion::KDBX31 {
        return Err(DatabaseError::InvalidVersion(format!(
            "Expected KDBX 3.1, got {version:?}"
        )));
    }

    // 2. Read outer header
    let header = diagnostics.run(
        DiagnosticStage::OuterHeader,
        || {
            let mut tee = TeeReader::new(reader, &mut header_buf);
            DatabaseReader::read_kdbx31_header(&mut tee)
        },
        |_| None,
    )?;
    diagnostics.set_kdbx31_header(&header);

    // 3. Normalize KDF parameters, then derive the transformed key once.
    let params = kdf_parameters(&header);
    let composite_key = diagnostics.run(
        DiagnosticStage::KeyDerivation,
        || derive_composite_key(&params),
        |_| None,
    )?;
    let final_key = diagnostics.run(
        DiagnosticStage::KeyDerivation,
        || derive_kdbx31_key(&composite_key, &header, &params),
        |_| None,
    )?;

    // 4. Read encrypted payload.
    //
    // KDBX 3.1 has no per-block integrity framing like KDBX 4's HMAC stream,
    // so we must read the remaining bytes in one shot. A sanity cap guards
    // against pathological / malicious inputs that would otherwise let a
    // caller OOM the process via `read_to_end`. 1 GiB is well above any
    // realistic KeePass database while still bounded.
    const KDBX31_MAX_ENCRYPTED_PAYLOAD: usize = 1024 * 1024 * 1024;
    let encrypted = diagnostics.run(
        DiagnosticStage::PayloadRead,
        || {
            let mut encrypted = Vec::new();
            let mut buf = [0u8; 16384];
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                if encrypted.len().saturating_add(n) > KDBX31_MAX_ENCRYPTED_PAYLOAD {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "Encrypted payload exceeds sanity cap of {KDBX31_MAX_ENCRYPTED_PAYLOAD} bytes"
                    )));
                }
                encrypted.extend_from_slice(&buf[..n]);
            }
            Ok(encrypted)
        },
        |encrypted| Some(format!("{} bytes", encrypted.len())),
    )?;

    // 5. Decrypt
    let cipher = create_cipher_engine(header.encryption_algorithm);
    let decrypted = diagnostics.run(
        DiagnosticStage::Decryption,
        || {
            Ok(Zeroizing::new(final_key.unlock(|key| {
                cipher
                    .decrypt(key, &header.encryption_iv, &encrypted)
                    .map_err(DatabaseError::from_decryption_error)
            })??))
        },
        |decrypted| Some(format!("{} bytes", decrypted.len())),
    )?;

    // 6. Verify stream start bytes
    diagnostics.run(
        DiagnosticStage::CredentialAuthentication,
        || {
            if decrypted.len() < 32 {
                return Err(DatabaseError::DecryptionError(
                    "Decrypted data too short".into(),
                ));
            }
            if header.stream_start_bytes.len() != 32 {
                return Err(DatabaseError::InvalidFormat(
                    "KDBX 3.1 stream start bytes must be 32 bytes".into(),
                ));
            }
            if decrypted[..32] != header.stream_start_bytes {
                return Err(DatabaseError::InvalidKey);
            }
            Ok(())
        },
        |_| None,
    )?;

    // 7. Read hashed blocks → compressed XML
    let block_data = &decrypted[32..];
    let mut block_reader = HashedBlockReader::new(std::io::Cursor::new(block_data));
    let compressed = diagnostics.run(
        DiagnosticStage::PayloadIntegrity,
        || block_reader.read_all_sensitive(),
        |compressed| Some(format!("{} bytes", compressed.len())),
    )?;

    // 8. Decompress
    let xml_data = diagnostics.run(
        DiagnosticStage::Decompression,
        || match header.compression {
            CompressionAlgorithm::Gzip => decompress_sensitive(compressed.as_slice()),
            CompressionAlgorithm::None => Ok(compressed),
        },
        |xml| Some(format!("{} bytes", xml.len())),
    )?;

    // 9. Verify the hash stored in Meta/HeaderHash before parsing the database.
    diagnostics.run(
        DiagnosticStage::HeaderHash,
        || verify_kdbx31_header_hash(xml_data.as_slice(), &header_buf),
        |_| None,
    )?;

    diagnostics.write_xml(xml_data.as_slice())?;

    // 10. Parse XML with inner stream protection
    let mut inner_stream = diagnostics.run(
        DiagnosticStage::InnerProtection,
        || create_inner_stream(header.inner_random_stream, &header.inner_random_stream_key),
        |_| None,
    )?;
    let mut database = diagnostics.run(
        DiagnosticStage::XmlParse,
        || {
            let xml_str = std::str::from_utf8(xml_data.as_slice())
                .map_err(|e| DatabaseError::InvalidFormat(format!("XML not UTF-8: {e}")))?;
            KdbxXmlReader::read(xml_str, inner_stream.as_mut())
        },
        |database| {
            Some(format!(
                "{} groups, {} entries",
                database.groups.len(),
                database.entries.len()
            ))
        },
    )?;

    // 11. Populate database metadata from header
    database.version = DatabaseVersion::KDBX31;
    database.encryption_algorithm = header.encryption_algorithm;
    database.compression = header.compression;
    database.file_version = header.version;
    database.kdf_parameters = Some(params);
    database.loaded = true;

    Ok((database, composite_key))
}

fn verify_kdbx31_header_hash(xml_data: &[u8], header: &[u8]) -> DatabaseResult<()> {
    let xml = std::str::from_utf8(xml_data)
        .map_err(|e| DatabaseError::InvalidFormat(format!("XML not UTF-8: {e}")))?;
    let Some(stored_hash) = read_kdbx31_header_hash(xml)? else {
        return Ok(());
    };
    let expected_hash = HashEngine::sha256(header);
    if stored_hash != expected_hash {
        return Err(DatabaseError::InvalidFormat("Header hash mismatch".into()));
    }
    Ok(())
}

fn read_kdbx31_header_hash(xml: &str) -> DatabaseResult<Option<[u8; 32]>> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Zeroizing::new(Vec::new());
    let mut depth = 0usize;
    let mut in_meta = false;
    let mut header_hash = None;

    loop {
        buf.clear();
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => {
                let element_name = e.name();
                let name = std::str::from_utf8(element_name.as_ref()).unwrap_or("");
                if in_meta && depth == 2 && name == "HeaderHash" {
                    let value = read_kdbx31_text_content(&mut reader, &mut buf)?;
                    let decoded = decode_kdbx31_header_hash(&value)?;
                    if header_hash.replace(decoded).is_some() {
                        return Err(DatabaseError::InvalidFormat(
                            "duplicate Meta/HeaderHash element".into(),
                        ));
                    }
                    continue;
                }
                if depth == 1 && name == "Meta" {
                    in_meta = true;
                }
                depth = depth.checked_add(1).ok_or_else(|| {
                    DatabaseError::InvalidFormat("XML nesting depth overflow".into())
                })?;
            }
            Event::Empty(e) if in_meta && depth == 2 => {
                if std::str::from_utf8(e.name().as_ref()).unwrap_or("") == "HeaderHash" {
                    return Err(DatabaseError::InvalidFormat(
                        "Meta/HeaderHash value is empty".into(),
                    ));
                }
            }
            Event::End(e) => {
                let element_name = e.name();
                let name = std::str::from_utf8(element_name.as_ref()).unwrap_or("");
                if in_meta && depth == 2 && name == "HeaderHash" {
                    continue;
                }
                if in_meta && depth == 2 && name == "Meta" {
                    return Ok(header_hash);
                }
                depth = depth.checked_sub(1).ok_or_else(|| {
                    DatabaseError::InvalidFormat("Unbalanced XML end element".into())
                })?;
            }
            Event::Eof => return Ok(header_hash),
            _ => {}
        }
    }
}

fn read_kdbx31_text_content<R: BufRead>(
    reader: &mut Reader<R>,
    buf: &mut Vec<u8>,
) -> DatabaseResult<String> {
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Text(text) => {
                return Ok(text
                    .unescape()
                    .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?
                    .into_owned())
            }
            Event::CData(value) => {
                return std::str::from_utf8(value.as_ref())
                    .map(str::to_owned)
                    .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))
            }
            Event::End(_) => return Ok(String::new()),
            Event::Start(_) | Event::Empty(_) => {
                return Err(DatabaseError::InvalidFormat(
                    "Meta/HeaderHash contains nested elements".into(),
                ))
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of Meta/HeaderHash element".into(),
                ))
            }
            _ => {}
        }
    }
}

fn decode_kdbx31_header_hash(value: &str) -> DatabaseResult<[u8; 32]> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .map_err(|err| {
            DatabaseError::InvalidFormat(format!("invalid Meta/HeaderHash base64: {err}"))
        })?;
    decoded
        .try_into()
        .map_err(|_| DatabaseError::InvalidFormat("Meta/HeaderHash must be 32 bytes".into()))
}

/// Derive the final encryption key for KDBX 3.1 using AES-KDF.
fn derive_kdbx31_key(
    composite_key: &CompositeKey,
    header: &KdbxHeader31,
    params: &KdfParameters,
) -> DatabaseResult<SecureArray<32>> {
    if !composite_key.matches(params) {
        return Err(DatabaseError::KdfParametersMismatch);
    }
    let mut final_key = composite_key
        .with_key(|transformed| HashEngine::sha256_multi(&[&header.master_seed, transformed]))?;
    Ok(SecureArray::from_array_mut(&mut final_key)?)
}

fn kdf_parameters(header: &KdbxHeader31) -> KdfParameters {
    let mut params = KdfParameters::new(AES_KDF_UUID);
    params.set_uuid_param();
    params.set_byte_array("S", &header.transform_seed);
    params.set_uint64("R", header.transform_rounds);
    params
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::db::composite_key::CompositeKey;
    use crate::SecureArray;

    #[test]
    fn test_invalid_signature_rejected() {
        let mut data = Vec::new();
        data.extend_from_slice(&0xDEADBEEFu32.to_le_bytes());
        data.extend_from_slice(&0xDEADBEEFu32.to_le_bytes());
        data.extend_from_slice(&0x00030001u32.to_le_bytes());

        let key =
            CompositeKey::from_derived_key(SecureArray::from_slice(&[0; 32]).unwrap(), [0; 32]);
        let mut cursor = std::io::Cursor::new(data);
        let result = read_kdbx31(&mut cursor, &key);
        assert!(result.is_err());
    }
}
