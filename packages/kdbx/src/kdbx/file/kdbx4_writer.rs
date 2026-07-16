//! KDBX 4.0 complete write pipeline
//!
//! Pipeline: Database → XML → compress → inner header + encrypted data
//!           → HMAC block stream → header HMAC → outer header → signature

use std::io::Write;

use byteorder::{LittleEndian, WriteBytesExt};

use crate::crypto::compression::CompressionAlgorithm;
use crate::crypto::inner_stream::create_inner_stream;
use crate::model::db::composite_key::CompositeKey;
use crate::model::db::database::Database;
use crate::model::exception::{DatabaseError, DatabaseResult};
use crate::kdbx::file::header::{
    CrsAlgorithm, FILE_VERSION_4, header_field_4, inner_header_field_4,
    KDBX_SIGNATURE_1, KDBX_SIGNATURE_2,
};
use crate::kdbx::kdf::create_kdf;
use crate::kdbx::stream::hmac_block_stream::{
    write_hmac_block_stream, compute_header_hmac,
};
use crate::kdbx::xml::KdbxXmlWriter;

/// Write a KDBX 4.0 database to a writer.
pub fn write_kdbx4<W: Write>(
    writer: &mut W,
    database: &Database,
    composite_key: &CompositeKey,
) -> DatabaseResult<()> {
    // 1. Generate header parameters
    let master_seed = generate_random_bytes(32);
    let encryption_iv = generate_random_bytes(database.encryption_algorithm.iv_length());
    let inner_stream_key = generate_random_bytes(32);

    // Get KDF parameters (use existing or default)
    let kdf_uuid = database.kdf_parameters.as_ref()
        .map(|p| p.kdf_uuid)
        .unwrap_or_else(|| crate::kdbx::kdf::argon2_kdf::ARGON2ID_UUID);
    let kdf = create_kdf(&kdf_uuid)
        .ok_or_else(|| DatabaseError::InvalidFormat("Unknown KDF".into()))?;
    let mut kdf_params = database.kdf_parameters.clone()
        .unwrap_or_else(|| kdf.default_parameters());
    kdf.randomize(&mut kdf_params);

    // 2. Derive master key
    let raw_key = composite_key.build_raw_key();
    if raw_key.is_empty() {
        return Err(DatabaseError::InvalidKey);
    }
    let transformed = kdf.transform(&raw_key, &kdf_params)?;
    let mut combined = Vec::with_capacity(master_seed.len() + transformed.len());
    combined.extend_from_slice(&master_seed);
    combined.extend_from_slice(&transformed);
    let master_key = crate::crypto::HashEngine::sha256(&combined).to_vec();

    // 3. Serialize XML with inner stream
    let mut inner_stream = create_inner_stream(CrsAlgorithm::ChaCha20, &inner_stream_key)?;
    let xml = KdbxXmlWriter::write(database, inner_stream.as_mut())?;
    let xml_bytes = xml.into_bytes();

    // 4. Compress
    let compressed = match database.compression {
        CompressionAlgorithm::Gzip => crate::crypto::compression::compress(&xml_bytes)?,
        CompressionAlgorithm::None => xml_bytes,
    };

    // 5. Build inner header + compressed data
    let mut plaintext = Vec::new();
    write_inner_header(&mut plaintext, &inner_stream_key)?;
    plaintext.extend_from_slice(&compressed);

    // 6. Encrypt
    let cipher = crate::crypto::cipher_engine::create_cipher_engine(database.encryption_algorithm);
    let encrypted = cipher.encrypt(&master_key, &encryption_iv, &plaintext)
        .map_err(|e| DatabaseError::EncryptionError(e.to_string()))?;

    // 7. Write outer header (capturing bytes for HMAC)
    let mut header_buf = Vec::new();
    write_signature(&mut header_buf)?;
    write_outer_header(&mut header_buf, database, &master_seed, &encryption_iv, &kdf_params)?;

    // 8. Compute header HMAC
    let header_hmac = compute_header_hmac(&master_key, &header_buf)?;

    // 9. Write HMAC block stream
    let mut hmac_stream = Vec::new();
    write_hmac_block_stream(&mut hmac_stream, &master_key, &encrypted)?;

    // 10. Write everything
    writer.write_all(&header_buf)?;
    writer.write_all(&header_hmac)?;
    writer.write_all(&hmac_stream)?;

    Ok(())
}

fn write_signature<W: Write>(w: &mut W) -> DatabaseResult<()> {
    w.write_u32::<LittleEndian>(KDBX_SIGNATURE_1)?;
    w.write_u32::<LittleEndian>(KDBX_SIGNATURE_2)?;
    w.write_u32::<LittleEndian>(FILE_VERSION_4)?;
    Ok(())
}

fn write_outer_header<W: Write>(
    w: &mut W,
    db: &Database,
    master_seed: &[u8],
    encryption_iv: &[u8],
    kdf_params: &crate::kdbx::kdf::kdf_parameters::KdfParameters,
) -> DatabaseResult<()> {
    let uuid_bytes = *db.encryption_algorithm.uuid().as_bytes();
    write_header_field_4(w, header_field_4::CIPHER_ID, &uuid_bytes)?;
    write_header_field_4(w, header_field_4::COMPRESSION_FLAGS, &db.compression.to_id().to_le_bytes())?;
    write_header_field_4(w, header_field_4::MASTER_SEED, master_seed)?;
    write_header_field_4(w, header_field_4::ENCRYPTION_IV, encryption_iv)?;
    let kdf_bytes = kdf_params.serialize();
    write_header_field_4(w, header_field_4::KDF_PARAMETERS, &kdf_bytes)?;
    write_header_field_4(w, header_field_4::END_OF_HEADER, &[])?;
    Ok(())
}

fn write_inner_header<W: Write>(w: &mut W, inner_stream_key: &[u8]) -> DatabaseResult<()> {
    let crs_id = (CrsAlgorithm::ChaCha20.to_id()).to_le_bytes();
    write_inner_field(w, inner_header_field_4::INNER_RANDOM_STREAM_ID, &crs_id)?;
    write_inner_field(w, inner_header_field_4::INNER_RANDOM_STREAM_KEY, inner_stream_key)?;
    write_inner_field(w, inner_header_field_4::END_OF_HEADER, &[])?;
    Ok(())
}

fn write_header_field_4<W: Write>(w: &mut W, field_id: u8, data: &[u8]) -> DatabaseResult<()> {
    w.write_u8(field_id)?;
    w.write_u32::<LittleEndian>(data.len() as u32)?;
    w.write_all(data)?;
    Ok(())
}

fn write_inner_field<W: Write>(w: &mut W, field_id: u8, data: &[u8]) -> DatabaseResult<()> {
    w.write_u8(field_id)?;
    w.write_u32::<LittleEndian>(data.len() as u32)?;
    w.write_all(data)?;
    Ok(())
}

fn generate_random_bytes(len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    getrandom::getrandom(&mut buf).unwrap_or_else(|_| {
        for b in buf.iter_mut() {
            *b = (chrono::Utc::now().timestamp_millis() & 0xFF) as u8;
        }
    });
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::db::database::DatabaseVersion;
    use crate::model::db::composite_key::CompositeKey;
    use crate::model::group::Group;
    use crate::model::core::node::NodeId;
    use crate::model::entry::Entry;
    use uuid::Uuid;

    #[test]
    fn test_write_read_roundtrip() {
        let mut db = Database::new(DatabaseVersion::KDBX4);
        let root_id = NodeId::from_uuid(Uuid::new_v4());
        let mut root = Group::new(root_id);
        root.title = "Root".to_string();

        let entry_id = NodeId::from_uuid(Uuid::new_v4());
        let mut entry = Entry::new(entry_id);
        entry.title = "KDBX4 Test".to_string();
        entry.password = crate::model::core::security::ProtectedString::new_protected("p@ssw0rd");

        root.add_child_entry(entry_id);
        db.entries.insert(entry_id, entry);
        db.groups.insert(root_id, root);
        db.root_group_id = Some(root_id);

        let key = CompositeKey::new().with_password(b"test_pass");

        // Write
        let mut buf = Vec::new();
        write_kdbx4(&mut buf, &db, &key).unwrap();
        assert!(!buf.is_empty());

        // Read back
        let mut cursor = std::io::Cursor::new(buf);
        let db2 = crate::kdbx::file::kdbx4_reader::read_kdbx4(&mut cursor, &key).unwrap();

        assert_eq!(db2.version, DatabaseVersion::KDBX4);
        assert_eq!(db2.entries.len(), 1);
        let e = db2.entries.values().next().unwrap();
        assert_eq!(e.title, "KDBX4 Test");
    }
}
