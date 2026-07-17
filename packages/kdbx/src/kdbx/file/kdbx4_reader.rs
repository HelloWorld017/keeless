//! KDBX 4.0 complete read pipeline
//!
//! Pipeline: signature → outer header → header HMAC → HMAC block stream
//!           → decrypt → inner header → decompress → inner stream → XML → Database

use std::io::Read;

use byteorder::{LittleEndian, ReadBytesExt};
use zeroize::{Zeroize, Zeroizing};

use crate::crypto::compression::CompressionAlgorithm;
use crate::crypto::inner_stream::create_inner_stream;
use crate::kdbx::file::header::{
    header_field_4, inner_header_field_4, CrsAlgorithm, KdbxHeader4, KdbxInnerHeader4,
    FILE_VERSION_4, KDBX_SIGNATURE_1, KDBX_SIGNATURE_2,
};
use crate::kdbx::kdf::create_kdf;
use crate::kdbx::limits::{
    MAX_INNER_HEADER_FIELD_SIZE, MAX_INNER_HEADER_SIZE, MAX_OUTER_HEADER_FIELD_SIZE,
    MAX_OUTER_HEADER_SIZE,
};
use crate::kdbx::stream::hmac_block_stream::{compute_header_hmac, read_hmac_block_stream};
use crate::kdbx::xml::KdbxXmlReader;
use crate::model::db::composite_key::CompositeKey;
use crate::model::db::database::{Database, DatabaseVersion};
use crate::model::exception::{DatabaseError, DatabaseResult};

/// Read a KDBX 4.0 database from a reader.
pub fn read_kdbx4<R: Read>(
    reader: &mut R,
    composite_key: &CompositeKey,
) -> DatabaseResult<Database> {
    // 1. Read and retain the exact version header. The minor version is part
    // of the authenticated header and cannot be reconstructed as 4.0.
    let signature1 = reader.read_u32::<LittleEndian>()?;
    let signature2 = reader.read_u32::<LittleEndian>()?;
    let raw_version = reader.read_u32::<LittleEndian>()?;
    if signature1 != KDBX_SIGNATURE_1 || signature2 != KDBX_SIGNATURE_2 {
        return Err(DatabaseError::InvalidSignature(
            "Expected KDBX signature".into(),
        ));
    }
    if raw_version >> 16 != 4 {
        return Err(DatabaseError::InvalidVersion(format!(
            "Expected KDBX4, got {raw_version:#010x}"
        )));
    }

    // 2. Read outer header (capturing bytes for HMAC verification)
    // Note: Writer computes HMAC over signature(12) + outer_header.
    // detect_version consumed 12 bytes of signature, so we prepend them.
    let mut header_buf = Vec::new();
    header_buf.extend_from_slice(&signature1.to_le_bytes());
    header_buf.extend_from_slice(&signature2.to_le_bytes());
    header_buf.extend_from_slice(&raw_version.to_le_bytes());
    let mut header = {
        let mut tee = TeeReader::new(reader, &mut header_buf);
        read_kdbx4_outer_header_from(&mut tee)?
    };
    header.version = raw_version;

    // 3. Verify the unkeyed header hash before doing expensive KDF work.
    let mut stored_hash = [0u8; 32];
    reader.read_exact(&mut stored_hash)?;
    let expected_hash = crate::crypto::HashEngine::sha256(&header_buf);
    if stored_hash != expected_hash {
        return Err(DatabaseError::InvalidFormat("Header hash mismatch".into()));
    }

    // 4. Derive the separate cipher and HMAC keys.
    let (master_key, hmac_key) = derive_keys(composite_key, &header)?;

    // 5. Verify header HMAC (next 32 bytes)
    let mut stored_hmac = [0u8; 32];
    reader.read_exact(&mut stored_hmac)?;
    let expected_hmac = compute_header_hmac(hmac_key.as_slice(), &header_buf)?;
    if stored_hmac != expected_hmac {
        return Err(DatabaseError::InvalidCredentials);
    }

    // 6. Read HMAC block stream → encrypted data
    let encrypted = read_hmac_block_stream(reader, hmac_key.as_slice())?;

    // 7. Decrypt and decompress the complete payload.
    let cipher = crate::crypto::cipher_engine::create_cipher_engine(header.encryption_algorithm);
    let decrypted = Zeroizing::new(
        cipher
            .decrypt(master_key.as_slice(), &header.encryption_iv, &encrypted)
            .map_err(|e| DatabaseError::DecryptionError(e.to_string()))?,
    );
    let payload = match header.compression {
        CompressionAlgorithm::Gzip => {
            crate::crypto::compression::decompress_sensitive(decrypted.as_slice())?
        }
        CompressionAlgorithm::None => decrypted,
    };

    // 8. Parse inner header.
    let mut cursor = std::io::Cursor::new(payload.as_slice());
    let mut inner = read_kdbx4_inner_header(&mut cursor)?;

    // 9. Parse XML with inner stream cipher
    let inner_stream_key = Zeroizing::new(std::mem::take(&mut inner.inner_random_stream_key));
    let mut inner_stream =
        create_inner_stream(inner.inner_random_stream, inner_stream_key.as_slice())?;
    let inner_binaries = std::mem::take(&mut inner.binaries);
    let mut binaries = SensitiveBinaries(Vec::with_capacity(inner_binaries.len()));
    for mut binary in inner_binaries {
        let protected = binary.is_protected();
        if protected {
            inner_stream.process(&mut binary.data);
        }
        binaries.0.push((binary.data, protected));
    }
    let xml_bytes = &payload[cursor.position() as usize..];
    let xml_str =
        std::str::from_utf8(xml_bytes).map_err(|e| DatabaseError::InvalidFormat(e.to_string()))?;
    let mut db = KdbxXmlReader::read_with_binaries(xml_str, inner_stream.as_mut(), &binaries.0)?;
    db.version = DatabaseVersion::KDBX4;
    db.file_version = header.version;
    db.encryption_algorithm = header.encryption_algorithm;
    db.compression = header.compression;
    db.kdf_parameters = header.kdf_parameters;
    db.public_custom_data = header.public_custom_data;
    db.header_comment = header.comment;

    Ok(db)
}

/// Derive the encryption key and HMAC base key defined by KDBX4.
type DerivedKeys = (Zeroizing<[u8; 32]>, Zeroizing<[u8; 64]>);

fn derive_keys(composite_key: &CompositeKey, header: &KdbxHeader4) -> DatabaseResult<DerivedKeys> {
    let raw_key = Zeroizing::new(composite_key.build_raw_key());
    if raw_key.is_empty() {
        return Err(DatabaseError::InvalidKey);
    }

    let kdf_uuid = header
        .kdf_parameters
        .as_ref()
        .map(|p| p.kdf_uuid)
        .ok_or_else(|| DatabaseError::InvalidFormat("No KDF parameters".into()))?;

    let kdf =
        create_kdf(&kdf_uuid).ok_or_else(|| DatabaseError::InvalidFormat("Unknown KDF".into()))?;
    let params = header
        .kdf_parameters
        .as_ref()
        .ok_or_else(|| DatabaseError::InvalidFormat("No KDF parameters".into()))?;

    let transformed = Zeroizing::new(kdf.transform(raw_key.as_slice(), params)?);
    let master_key = Zeroizing::new(crate::crypto::HashEngine::sha256_multi(&[
        &header.master_seed,
        transformed.as_slice(),
    ]));
    let hmac_key = Zeroizing::new(crate::crypto::HashEngine::sha512_multi(&[
        &header.master_seed,
        transformed.as_slice(),
        &[0x01],
    ]));
    Ok((master_key, hmac_key))
}

/// Read KDBX 4.0 outer header from reader, TeeReader captures all bytes.
fn read_kdbx4_outer_header_from<R: Read>(reader: &mut R) -> DatabaseResult<KdbxHeader4> {
    let mut cipher = None;
    let mut compression = None;
    let mut master_seed = None;
    let mut encryption_iv = None;
    let mut kdf_parameters = None;
    let mut public_custom_data = Vec::new();
    let mut comment = None;
    let mut saw_public_custom_data = false;
    let mut total_size = 0usize;
    let mut header = KdbxHeader4 {
        version: FILE_VERSION_4,
        comment: None,
        encryption_algorithm: crate::crypto::encryption_algorithm::EncryptionAlgorithm::AesRijndael,
        compression: CompressionAlgorithm::Gzip,
        master_seed: Vec::new(),
        encryption_iv: Vec::new(),
        kdf_parameters: None,
        public_custom_data: Vec::new(),
    };

    loop {
        let field_id = reader.read_u8()?;
        let field_size = usize::try_from(reader.read_u32::<LittleEndian>()?).map_err(|_| {
            DatabaseError::InvalidFormat("Outer header field size is not representable".into())
        })?;
        total_size = total_size
            .checked_add(5)
            .and_then(|n| n.checked_add(field_size))
            .ok_or_else(|| DatabaseError::InvalidFormat("Outer header size overflow".into()))?;
        if field_size > MAX_OUTER_HEADER_FIELD_SIZE || total_size > MAX_OUTER_HEADER_SIZE {
            return Err(DatabaseError::InvalidFormat(
                "Outer header exceeds resource limits".into(),
            ));
        }
        if field_id == header_field_4::END_OF_HEADER {
            if field_size != 4 {
                return Err(DatabaseError::InvalidFormat(
                    "KDBX4 end header field must contain the four-byte marker".into(),
                ));
            }
            let mut marker = [0u8; 4];
            reader.read_exact(&mut marker)?;
            if marker != [0x0D, 0x0A, 0x0D, 0x0A] {
                return Err(DatabaseError::InvalidFormat(
                    "Invalid KDBX4 end header marker".into(),
                ));
            }
            break;
        }

        let mut data = vec![0u8; field_size];
        reader.read_exact(&mut data)?;
        match field_id {
            header_field_4::CIPHER_ID => {
                if data.len() != 16 || cipher.is_some() {
                    return Err(DatabaseError::InvalidFormat(
                        "Invalid or duplicate CIPHER_ID".into(),
                    ));
                }
                let uuid = uuid::Uuid::from_slice(&data)
                    .map_err(|e| DatabaseError::InvalidFormat(e.to_string()))?;
                cipher = Some(
                    crate::crypto::encryption_algorithm::EncryptionAlgorithm::from_uuid(&uuid)
                        .ok_or_else(|| {
                            DatabaseError::InvalidFormat("Unknown KDBX4 cipher ID".into())
                        })?,
                );
            }
            header_field_4::COMPRESSION_FLAGS => {
                if data.len() != 4 || compression.is_some() {
                    return Err(DatabaseError::InvalidFormat(
                        "Invalid or duplicate COMPRESSION_FLAGS".into(),
                    ));
                }
                let flags = u32::from_le_bytes(data.try_into().map_err(|_| {
                    DatabaseError::InvalidFormat("Invalid COMPRESSION_FLAGS".into())
                })?);
                compression = Some(CompressionAlgorithm::from_id(flags).ok_or_else(|| {
                    DatabaseError::InvalidFormat("Unknown compression ID".into())
                })?);
            }
            header_field_4::MASTER_SEED => {
                if data.len() != 32 || master_seed.is_some() {
                    return Err(DatabaseError::InvalidFormat(
                        "Invalid or duplicate MASTER_SEED".into(),
                    ));
                }
                master_seed = Some(data);
            }
            header_field_4::ENCRYPTION_IV => {
                if encryption_iv.is_some() {
                    return Err(DatabaseError::InvalidFormat(
                        "Duplicate ENCRYPTION_IV".into(),
                    ));
                }
                encryption_iv = Some(data);
            }
            header_field_4::KDF_PARAMETERS => {
                if kdf_parameters.is_some() {
                    return Err(DatabaseError::InvalidFormat(
                        "Duplicate KDF_PARAMETERS".into(),
                    ));
                }
                kdf_parameters = Some(
                    crate::kdbx::kdf::kdf_parameters::KdfParameters::deserialize(&data)
                        .ok_or_else(|| {
                            DatabaseError::InvalidFormat("Malformed KDF parameters".into())
                        })?,
                );
            }
            header_field_4::PUBLIC_CUSTOM_DATA => {
                if saw_public_custom_data {
                    return Err(DatabaseError::InvalidFormat(
                        "Duplicate PUBLIC_CUSTOM_DATA".into(),
                    ));
                }
                saw_public_custom_data = true;
                public_custom_data = data;
            }
            header_field_4::COMMENT => {
                if comment.is_some() {
                    return Err(DatabaseError::InvalidFormat(
                        "Duplicate COMMENT header".into(),
                    ));
                }
                comment = Some(data);
            }
            _ => {
                return Err(DatabaseError::InvalidFormat(format!(
                    "Unknown KDBX4 outer header field {field_id}"
                )))
            }
        }
    }

    header.encryption_algorithm =
        cipher.ok_or_else(|| DatabaseError::InvalidFormat("Missing CIPHER_ID".into()))?;
    header.compression = compression
        .ok_or_else(|| DatabaseError::InvalidFormat("Missing COMPRESSION_FLAGS".into()))?;
    header.master_seed =
        master_seed.ok_or_else(|| DatabaseError::InvalidFormat("Missing MASTER_SEED".into()))?;
    header.encryption_iv = encryption_iv
        .ok_or_else(|| DatabaseError::InvalidFormat("Missing ENCRYPTION_IV".into()))?;
    if header.encryption_iv.len() != header.encryption_algorithm.iv_length() {
        return Err(DatabaseError::InvalidFormat(
            "Invalid encryption IV length".into(),
        ));
    }
    header.kdf_parameters = Some(
        kdf_parameters
            .ok_or_else(|| DatabaseError::InvalidFormat("Missing KDF_PARAMETERS".into()))?,
    );
    header.public_custom_data = public_custom_data;
    header.comment = comment;

    Ok(header)
}

/// Read KDBX 4.0 inner header.
fn read_kdbx4_inner_header<R: Read>(reader: &mut R) -> DatabaseResult<KdbxInnerHeader4> {
    let mut stream_id = None;
    let mut stream_key = None;
    let mut total_size = 0usize;
    let mut inner = KdbxInnerHeader4 {
        inner_random_stream: CrsAlgorithm::ChaCha20,
        inner_random_stream_key: Vec::new(),
        binaries: Vec::new(),
    };

    loop {
        let field_id = reader.read_u8()?;
        let field_size = usize::try_from(reader.read_u32::<LittleEndian>()?).map_err(|_| {
            DatabaseError::InvalidFormat("Inner header field size is not representable".into())
        })?;
        total_size = total_size
            .checked_add(5)
            .and_then(|n| n.checked_add(field_size))
            .ok_or_else(|| DatabaseError::InvalidFormat("Inner header size overflow".into()))?;
        if field_size > MAX_INNER_HEADER_FIELD_SIZE || total_size > MAX_INNER_HEADER_SIZE {
            return Err(DatabaseError::InvalidFormat(
                "Inner header exceeds resource limits".into(),
            ));
        }
        if field_id == inner_header_field_4::END_OF_HEADER {
            if field_size != 0 {
                return Err(DatabaseError::InvalidFormat(
                    "KDBX4 inner end field must be empty".into(),
                ));
            }
            break;
        }
        let mut data = Zeroizing::new(vec![0u8; field_size]);
        reader.read_exact(&mut data)?;
        match field_id {
            inner_header_field_4::INNER_RANDOM_STREAM_ID => {
                if data.len() != 4 || stream_id.is_some() {
                    return Err(DatabaseError::InvalidFormat(
                        "Invalid or duplicate INNER_RANDOM_STREAM_ID".into(),
                    ));
                }
                let id = u32::from_le_bytes(data.as_slice().try_into().map_err(|_| {
                    DatabaseError::InvalidFormat("Invalid INNER_RANDOM_STREAM_ID".into())
                })?);
                stream_id = Some(CrsAlgorithm::from_id(id).ok_or_else(|| {
                    DatabaseError::InvalidFormat("Unknown inner stream ID".into())
                })?);
            }
            inner_header_field_4::INNER_RANDOM_STREAM_KEY => {
                if !matches!(data.len(), 32 | 64) || stream_key.is_some() {
                    return Err(DatabaseError::InvalidFormat(
                        "Invalid or duplicate INNER_RANDOM_STREAM_KEY".into(),
                    ));
                }
                stream_key = Some(data.to_vec());
            }
            inner_header_field_4::BINARY => {
                if data.is_empty() {
                    return Err(DatabaseError::InvalidFormat(
                        "Inner binary field has no flags byte".into(),
                    ));
                }
                inner.binaries.push(crate::kdbx::file::header::KdbxBinary {
                    flags: data[0],
                    data: data[1..].to_vec(),
                });
            }
            _ => {
                return Err(DatabaseError::InvalidFormat(format!(
                    "Unknown KDBX4 inner header field {field_id}"
                )))
            }
        }
    }

    inner.inner_random_stream = stream_id
        .ok_or_else(|| DatabaseError::InvalidFormat("Missing INNER_RANDOM_STREAM_ID".into()))?;
    inner.inner_random_stream_key = stream_key
        .ok_or_else(|| DatabaseError::InvalidFormat("Missing INNER_RANDOM_STREAM_KEY".into()))?;

    Ok(inner)
}

struct SensitiveBinaries(Vec<(Vec<u8>, bool)>);

impl Drop for SensitiveBinaries {
    fn drop(&mut self) {
        for (data, _) in &mut self.0 {
            data.zeroize();
        }
    }
}

/// TeeReader copies all read bytes to a sink buffer.
/// Borrows the inner reader instead of owning it.
struct TeeReader<'a, R> {
    inner: &'a mut R,
    sink: &'a mut Vec<u8>,
}

impl<'a, R: Read> TeeReader<'a, R> {
    fn new(inner: &'a mut R, sink: &'a mut Vec<u8>) -> Self {
        Self { inner, sink }
    }
}

impl<'a, R: Read> Read for TeeReader<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.sink.extend_from_slice(&buf[..n]);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::db::composite_key::CompositeKey;

    #[test]
    fn test_invalid_version_rejected() {
        // Write KDBX 3.1 signature
        let mut data = Vec::new();
        data.extend_from_slice(&crate::kdbx::file::header::KDBX_SIGNATURE_1.to_le_bytes());
        data.extend_from_slice(&crate::kdbx::file::header::KDBX_SIGNATURE_2.to_le_bytes());
        data.extend_from_slice(&0x00030001u32.to_le_bytes()); // v3.1

        let mut cursor = std::io::Cursor::new(data);
        let key = CompositeKey::new().with_password(b"test");
        assert!(read_kdbx4(&mut cursor, &key).is_err());
    }

    #[test]
    fn test_outer_header_rejects_unknown_and_oversized_fields() {
        let unknown = [99, 0, 0, 0, 0];
        assert!(matches!(
            read_kdbx4_outer_header_from(&mut &unknown[..]),
            Err(DatabaseError::InvalidFormat(_))
        ));

        let mut oversized = vec![header_field_4::COMMENT];
        oversized.extend_from_slice(&((MAX_OUTER_HEADER_FIELD_SIZE as u32) + 1).to_le_bytes());
        assert!(matches!(
            read_kdbx4_outer_header_from(&mut &oversized[..]),
            Err(DatabaseError::InvalidFormat(_))
        ));
    }

    #[test]
    fn test_inner_header_rejects_unknown_stream_id() {
        let mut data = vec![inner_header_field_4::INNER_RANDOM_STREAM_ID];
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(&99u32.to_le_bytes());
        assert!(matches!(
            read_kdbx4_inner_header(&mut &data[..]),
            Err(DatabaseError::InvalidFormat(_))
        ));
    }
}
