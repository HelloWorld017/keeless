//! KDBX 3.1 complete write pipeline
//!
//! Pipeline: Database → XML → compress → hashed block stream → encrypt
//!           → stream start bytes → outer header → signature

use std::io::Write;

use byteorder::{LittleEndian, WriteBytesExt};

use crate::crypto::compression::CompressionAlgorithm;
use crate::crypto::inner_stream::create_inner_stream;
use crate::kdbx::file::header::{
    header_field_31, CrsAlgorithm, KdbxHeader31, FILE_VERSION_31, KDBX_SIGNATURE_1,
    KDBX_SIGNATURE_2,
};
use crate::kdbx::kdf::aes_kdf::{AesKdf, AES_KDF_UUID};
use crate::kdbx::kdf::kdf_engine::KdfEngine;
use crate::kdbx::kdf::kdf_parameters::KdfParameters;
use crate::kdbx::stream::hashed_block::HashedBlockWriter;
use crate::kdbx::xml::KdbxXmlWriter;
use crate::model::db::composite_key::CompositeKey;
use crate::model::db::database::Database;
use crate::model::exception::{DatabaseError, DatabaseResult};

/// Write a KDBX 3.1 database to a writer.
pub fn write_kdbx31<W: Write>(
    writer: &mut W,
    database: &Database,
    composite_key: &CompositeKey,
) -> DatabaseResult<()> {
    // 1. Generate header parameters
    let master_seed = generate_random_bytes(32)?;
    let transform_seed = generate_random_bytes(32)?;
    let encryption_iv = generate_random_bytes(database.encryption_algorithm.iv_length())?;
    let inner_stream_key = generate_random_bytes(32)?;
    let transform_rounds: u64 = 1_000;

    let header = KdbxHeader31 {
        version: FILE_VERSION_31,
        encryption_algorithm: database.encryption_algorithm,
        compression: database.compression,
        master_seed: master_seed.clone(),
        transform_seed: transform_seed.clone(),
        transform_rounds,
        encryption_iv: encryption_iv.clone(),
        inner_random_stream_key: inner_stream_key.clone(),
        stream_start_bytes: Vec::new(),
        inner_random_stream: CrsAlgorithm::Salsa20,
    };

    // 2. Derive final key
    let final_key = derive_key(
        composite_key,
        &master_seed,
        &transform_seed,
        transform_rounds,
    )?;

    // 3. Serialize database to XML with inner stream protection
    let mut inner_stream = create_inner_stream(CrsAlgorithm::Salsa20, &inner_stream_key)?;
    let xml = KdbxXmlWriter::write(database, inner_stream.as_mut())?;

    // 4. Compress
    let xml_bytes = xml.into_bytes();
    let compressed = match database.compression {
        CompressionAlgorithm::Gzip => crate::crypto::compression::compress(&xml_bytes)?,
        CompressionAlgorithm::None => xml_bytes,
    };

    // 5. Wrap in hashed blocks
    let mut hashed_blocks = Vec::new();
    {
        let mut hbw = HashedBlockWriter::new(&mut hashed_blocks);
        hbw.write_all(&compressed)?;
    }

    // 6. Prepend stream start bytes (SHA-256 of final key)
    let stream_start = crate::crypto::HashEngine::sha256(&final_key);
    let mut plaintext = Vec::with_capacity(32 + hashed_blocks.len());
    plaintext.extend_from_slice(&stream_start);
    plaintext.extend_from_slice(&hashed_blocks);

    // 7. Encrypt
    let cipher = crate::crypto::cipher_engine::create_cipher_engine(database.encryption_algorithm);
    let encrypted = cipher
        .encrypt(&final_key, &encryption_iv, &plaintext)
        .map_err(|e| DatabaseError::EncryptionError(e.to_string()))?;

    // 8. Write file: signature + header + encrypted data
    write_signature(writer)?;
    write_kdbx31_header(writer, &header)?;
    writer.write_all(&encrypted)?;

    Ok(())
}

fn derive_key(
    composite_key: &CompositeKey,
    master_seed: &[u8],
    transform_seed: &[u8],
    transform_rounds: u64,
) -> DatabaseResult<Vec<u8>> {
    let raw_key = composite_key.build_raw_key();
    if raw_key.is_empty() {
        return Err(DatabaseError::InvalidKey);
    }

    let mut params = KdfParameters::new(AES_KDF_UUID);
    params.set_byte_array("S", transform_seed);
    params.set_uint64("R", transform_rounds);

    let kdf = AesKdf;
    let transformed = kdf.transform(&raw_key, &params)?;

    let mut combined = Vec::with_capacity(master_seed.len() + transformed.len());
    combined.extend_from_slice(master_seed);
    combined.extend_from_slice(&transformed);
    Ok(crate::crypto::HashEngine::sha256(&combined).to_vec())
}

fn write_signature<W: Write>(writer: &mut W) -> DatabaseResult<()> {
    writer.write_u32::<LittleEndian>(KDBX_SIGNATURE_1)?;
    writer.write_u32::<LittleEndian>(KDBX_SIGNATURE_2)?;
    writer.write_u32::<LittleEndian>(FILE_VERSION_31)?;
    Ok(())
}

fn write_kdbx31_header<W: Write>(writer: &mut W, header: &KdbxHeader31) -> DatabaseResult<()> {
    let uuid_bytes = *header.encryption_algorithm.uuid().as_bytes();
    write_header_field(writer, header_field_31::CIPHER_ID, &uuid_bytes)?;
    write_header_field(
        writer,
        header_field_31::COMPRESSION_FLAGS,
        &header.compression.to_id().to_le_bytes(),
    )?;
    write_header_field(writer, header_field_31::MASTER_SEED, &header.master_seed)?;
    write_header_field(
        writer,
        header_field_31::TRANSFORM_SEED,
        &header.transform_seed,
    )?;
    write_header_field(
        writer,
        header_field_31::TRANSFORM_ROUNDS,
        &header.transform_rounds.to_le_bytes(),
    )?;
    write_header_field(
        writer,
        header_field_31::ENCRYPTION_IV,
        &header.encryption_iv,
    )?;
    write_header_field(
        writer,
        header_field_31::INNER_RANDOM_STREAM_KEY,
        &header.inner_random_stream_key,
    )?;
    write_header_field(
        writer,
        header_field_31::STREAM_START_BYTES,
        &header.inner_random_stream_key,
    )?;
    write_header_field(
        writer,
        header_field_31::INNER_RANDOM_STREAM_ID,
        &header.inner_random_stream.to_id().to_le_bytes(),
    )?;
    write_header_field(writer, header_field_31::END_OF_HEADER, &[])?;
    Ok(())
}

fn write_header_field<W: Write>(writer: &mut W, field_id: u8, data: &[u8]) -> DatabaseResult<()> {
    writer.write_u8(field_id)?;
    writer.write_u16::<LittleEndian>(data.len() as u16)?;
    writer.write_all(data)?;
    Ok(())
}

fn generate_random_bytes(len: usize) -> DatabaseResult<Vec<u8>> {
    let mut buf = vec![0u8; len];
    getrandom::getrandom(&mut buf).map_err(|e| {
        DatabaseError::EncryptionError(format!("secure random generation failed: {e}"))
    })?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::core::node::NodeId;
    use crate::model::db::composite_key::CompositeKey;
    use crate::model::db::database::DatabaseVersion;
    use crate::model::entry::Entry;
    use crate::model::group::Group;
    use uuid::Uuid;

    #[test]
    fn test_write_read_roundtrip() {
        let mut db = Database::new(DatabaseVersion::KDBX31);
        let root_id = NodeId::from_uuid(Uuid::new_v4());
        let mut root = Group::new(root_id);
        root.title = "Root".to_string();

        let entry_id = NodeId::from_uuid(Uuid::new_v4());
        let mut entry = Entry::new(entry_id);
        entry.title = "Test".to_string();
        entry.password = crate::model::core::security::ProtectedString::new_protected("secret123");

        root.add_child_entry(entry_id);
        db.entries.insert(entry_id, entry);
        db.groups.insert(root_id, root);
        db.root_group_id = Some(root_id);

        let key = CompositeKey::new().with_password(b"test_password");

        // Write
        let mut buf = Vec::new();
        write_kdbx31(&mut buf, &db, &key).unwrap();
        assert!(!buf.is_empty());

        // Read back
        let mut cursor = std::io::Cursor::new(buf);
        let db2 = crate::kdbx::file::kdbx31_reader::read_kdbx31(&mut cursor, &key).unwrap();

        assert_eq!(db2.version, DatabaseVersion::KDBX31);
        assert_eq!(db2.entries.len(), 1);
        let e = db2.entries.values().next().unwrap();
        assert_eq!(e.title, "Test");
    }
}
