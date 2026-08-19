//! KDBX 3.1 complete read pipeline
//!
//! Pipeline: signature → outer header → key derivation → decrypt → verify streamStartBytes
//!           → hashed block stream → decompress → inner stream decrypt → XML → Database

use keeless_secure_types::{SecureArray, SecureBytes};
use std::io::Read;
use zeroize::Zeroizing;

use crate::crypto::cipher_engine::create_cipher_engine;
use crate::crypto::compression::{decompress_sensitive, CompressionAlgorithm};
use crate::crypto::inner_stream::create_inner_stream;
use crate::crypto::HashEngine;
use crate::kdbx::diagnostics::{DiagnosticContext, DiagnosticStage};
use crate::kdbx::file::header::KdbxHeader31;
use crate::kdbx::file::reader::{DatabaseReader, TeeReader};
use crate::kdbx::kdf::aes_kdf::{AesKdf, AES_KDF_UUID};
use crate::kdbx::kdf::kdf_engine::KdfEngine;
use crate::kdbx::kdf::kdf_parameters::KdfParameters;
use crate::kdbx::stream::hashed_block::HashedBlockReader;
use crate::kdbx::xml::KdbxXmlReader;
use crate::model::db::composite_key::CompositeKey;
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

pub(crate) fn read_kdbx31_diagnostic<R: Read>(
    reader: &mut R,
    composite_key: &CompositeKey,
    diagnostics: &mut DiagnosticContext<'_>,
) -> DatabaseResult<Database> {
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

    // 3. Derive final key
    let final_key = diagnostics.run(
        DiagnosticStage::KeyDerivation,
        || derive_kdbx31_key(composite_key, &header),
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

    diagnostics.write_xml(xml_data.as_slice())?;

    // 9. Parse XML with inner stream protection
    let mut inner_stream = diagnostics.run(
        DiagnosticStage::InnerProtection,
        || create_inner_stream(header.inner_random_stream, &header.inner_random_stream_key),
        |_| None,
    )?;
    let (mut database, header_hash) = diagnostics.run(
        DiagnosticStage::XmlParse,
        || {
            let xml_str = std::str::from_utf8(xml_data.as_slice())
                .map_err(|e| DatabaseError::InvalidFormat(format!("XML not UTF-8: {e}")))?;
            KdbxXmlReader::read_with_header_hash(xml_str, inner_stream.as_mut())
        },
        |(database, _)| {
            Some(format!(
                "{} groups, {} entries",
                database.groups.len(),
                database.entries.len()
            ))
        },
    )?;

    // 10. Verify the hash stored in Meta/HeaderHash.
    diagnostics.run(
        DiagnosticStage::HeaderHash,
        || {
            let Some(stored_hash) = header_hash.as_deref() else {
                return Ok(());
            };
            let expected_hash = HashEngine::sha256(&header_buf);
            if stored_hash != expected_hash.as_slice() {
                return Err(DatabaseError::InvalidFormat("Header hash mismatch".into()));
            }
            Ok(())
        },
        |_| None,
    )?;

    // 11. Populate database metadata from header
    database.version = DatabaseVersion::KDBX31;
    database.encryption_algorithm = header.encryption_algorithm;
    database.compression = header.compression;
    database.loaded = true;

    Ok(database)
}

/// Derive the final encryption key for KDBX 3.1 using AES-KDF.
fn derive_kdbx31_key(
    composite_key: &CompositeKey,
    header: &KdbxHeader31,
) -> DatabaseResult<SecureArray<32>> {
    let raw_key = composite_key.build_raw_key()?;

    // Build KDF parameters from header
    let mut params = KdfParameters::new(AES_KDF_UUID);
    params.set_byte_array("S", &header.transform_seed);
    params.set_uint64("R", header.transform_rounds);

    // Transform key with AES-KDF
    let kdf = AesKdf;
    let transformed = SecureBytes::from_vec(raw_key.unlock(|key| kdf.transform(key, &params))??)?;

    // Final key = SHA-256(masterSeed || transformedKey)
    let mut combined = Zeroizing::new(Vec::with_capacity(
        header.master_seed.len() + transformed.len(),
    ));
    combined.extend_from_slice(&header.master_seed);
    transformed.unlock_slice(|value| combined.extend_from_slice(value))?;
    let mut final_key = HashEngine::sha256(combined.as_slice());
    Ok(SecureArray::from_array_mut(&mut final_key)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::db::composite_key::CompositeKey;

    #[test]
    fn test_invalid_signature_rejected() {
        let mut data = Vec::new();
        data.extend_from_slice(&0xDEADBEEFu32.to_le_bytes());
        data.extend_from_slice(&0xDEADBEEFu32.to_le_bytes());
        data.extend_from_slice(&0x00030001u32.to_le_bytes());

        let key = CompositeKey::new().with_password(b"test").unwrap();
        let mut cursor = std::io::Cursor::new(data);
        let result = read_kdbx31(&mut cursor, &key);
        assert!(result.is_err());
    }
}
