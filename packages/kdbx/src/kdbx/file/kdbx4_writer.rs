//! KDBX 4.0 complete write pipeline
//!
//! Pipeline: Database → XML → compress → inner header + encrypted data
//!           → HMAC block stream → header HMAC → outer header → signature

use std::io::Write;

use byteorder::{LittleEndian, WriteBytesExt};
use secure_types::{SecureArray, SecureBytes};
use zeroize::Zeroizing;

use crate::crypto::compression::CompressionAlgorithm;
use crate::crypto::inner_stream::create_inner_stream;
use crate::kdbx::file::header::{
    header_field_4, inner_header_field_4, CrsAlgorithm, KDBX_SIGNATURE_1, KDBX_SIGNATURE_2,
};
use crate::kdbx::kdf::create_kdf;
use crate::kdbx::stream::hmac_block_stream::{compute_header_hmac, write_hmac_block_stream};
use crate::kdbx::xml::KdbxXmlWriter;
use crate::model::db::composite_key::CompositeKey;
use crate::model::db::database::Database;
use crate::model::exception::{DatabaseError, DatabaseResult};

/// Write a KDBX 4.0 database to a writer.
pub fn write_kdbx4<W: Write>(
    writer: &mut W,
    database: &Database,
    composite_key: &CompositeKey,
) -> DatabaseResult<()> {
    write_kdbx4_with_credentials(writer, database, composite_key, composite_key)
}

pub(crate) fn write_kdbx4_with_credentials<W: Write>(
    writer: &mut W,
    database: &Database,
    memory_key: &CompositeKey,
    file_key: &CompositeKey,
) -> DatabaseResult<()> {
    // 1. Generate header parameters
    let master_seed = generate_random_bytes(32)?;
    let encryption_iv = generate_random_bytes(database.encryption_algorithm.iv_length())?;
    let inner_stream_key = SecureBytes::from_vec(generate_random_bytes(32)?)?;

    // Get KDF parameters (use existing or default)
    let kdf_uuid = database
        .kdf_parameters
        .as_ref()
        .map(|p| p.kdf_uuid)
        .unwrap_or_else(|| crate::kdbx::kdf::argon2_kdf::ARGON2ID_UUID);
    let kdf =
        create_kdf(&kdf_uuid).ok_or_else(|| DatabaseError::InvalidFormat("Unknown KDF".into()))?;
    let mut kdf_params = database
        .kdf_parameters
        .clone()
        .unwrap_or_else(|| kdf.default_parameters());
    kdf.randomize(&mut kdf_params)?;

    // 2. Derive master key
    let raw_key = file_key.build_raw_key()?;
    let transformed =
        SecureBytes::from_vec(raw_key.unlock(|key| kdf.transform(key, &kdf_params))?)?;
    let mut master_key_bytes = transformed
        .unlock_slice(|value| crate::crypto::HashEngine::sha256_multi(&[&master_seed, value]));
    let mut hmac_key_bytes = transformed.unlock_slice(|value| {
        crate::crypto::HashEngine::sha512_multi(&[&master_seed, value, &[0x01]])
    });
    let master_key = SecureArray::from_array_mut(&mut master_key_bytes)?;
    let hmac_key = SecureArray::from_array_mut(&mut hmac_key_bytes)?;

    // 3. Serialize XML with inner stream
    let mut inner_stream =
        inner_stream_key.unlock_slice(|key| create_inner_stream(CrsAlgorithm::ChaCha20, key))?;
    let mut binaries = collect_binaries(database);
    for (data, protected) in &mut binaries {
        if *protected {
            inner_stream.process(data);
        }
    }
    let xml = KdbxXmlWriter::write_with_credentials(database, inner_stream.as_mut(), memory_key)?;
    let xml_bytes = Zeroizing::new(xml.into_bytes());

    // 4. Build and then compress the complete inner payload.
    let mut payload = Zeroizing::new(Vec::new());
    inner_stream_key.unlock_slice(|key| write_inner_header(&mut *payload, key, &binaries))?;
    payload.extend_from_slice(&xml_bytes);
    let plaintext = Zeroizing::new(match database.compression {
        CompressionAlgorithm::Gzip => crate::crypto::compression::compress(&payload)?,
        CompressionAlgorithm::None => payload.to_vec(),
    });

    // 6. Encrypt
    let cipher = crate::crypto::cipher_engine::create_cipher_engine(database.encryption_algorithm);
    let encrypted = master_key.unlock(|key| {
        cipher
            .encrypt(key, &encryption_iv, &plaintext)
            .map_err(DatabaseError::from_encryption_error)
    })?;

    // 7. Write outer header (capturing bytes for HMAC)
    let mut header_buf = Vec::new();
    write_signature(&mut header_buf, database.file_version)?;
    write_outer_header(
        &mut header_buf,
        database,
        &master_seed,
        &encryption_iv,
        &kdf_params,
    )?;

    // 8. Compute the unkeyed header hash and keyed header HMAC.
    let header_hash = crate::crypto::HashEngine::sha256(&header_buf);
    let header_hmac = hmac_key.unlock(|key| compute_header_hmac(key, &header_buf))?;

    // 9. Write HMAC block stream
    let mut hmac_stream = Vec::new();
    hmac_key.unlock(|key| write_hmac_block_stream(&mut hmac_stream, key, &encrypted))?;

    // 10. Write everything
    writer.write_all(&header_buf)?;
    writer.write_all(&header_hash)?;
    writer.write_all(&header_hmac)?;
    writer.write_all(&hmac_stream)?;

    Ok(())
}

fn write_signature<W: Write>(w: &mut W, version: u32) -> DatabaseResult<()> {
    if version >> 16 != 4 {
        return Err(DatabaseError::InvalidVersion(format!(
            "Expected KDBX4 version, got {version:#010x}"
        )));
    }
    w.write_u32::<LittleEndian>(KDBX_SIGNATURE_1)?;
    w.write_u32::<LittleEndian>(KDBX_SIGNATURE_2)?;
    w.write_u32::<LittleEndian>(version)?;
    Ok(())
}

fn write_outer_header<W: Write>(
    w: &mut W,
    db: &Database,
    master_seed: &[u8],
    encryption_iv: &[u8],
    kdf_params: &crate::kdbx::kdf::kdf_parameters::KdfParameters,
) -> DatabaseResult<()> {
    if let Some(comment) = &db.header_comment {
        write_header_field_4(w, header_field_4::COMMENT, comment)?;
    }
    let uuid_bytes = *db.encryption_algorithm.uuid().as_bytes();
    write_header_field_4(w, header_field_4::CIPHER_ID, &uuid_bytes)?;
    write_header_field_4(
        w,
        header_field_4::COMPRESSION_FLAGS,
        &db.compression.to_id().to_le_bytes(),
    )?;
    write_header_field_4(w, header_field_4::MASTER_SEED, master_seed)?;
    write_header_field_4(w, header_field_4::ENCRYPTION_IV, encryption_iv)?;
    let kdf_bytes = kdf_params.serialize();
    write_header_field_4(w, header_field_4::KDF_PARAMETERS, &kdf_bytes)?;
    if !db.public_custom_data.is_empty() {
        write_header_field_4(
            w,
            header_field_4::PUBLIC_CUSTOM_DATA,
            &db.public_custom_data,
        )?;
    }
    write_header_field_4(w, header_field_4::END_OF_HEADER, &[0x0D, 0x0A, 0x0D, 0x0A])?;
    Ok(())
}

fn write_inner_header<W: Write>(
    w: &mut W,
    inner_stream_key: &[u8],
    binaries: &[(Vec<u8>, bool)],
) -> DatabaseResult<()> {
    let crs_id = (CrsAlgorithm::ChaCha20.to_id()).to_le_bytes();
    write_inner_field(w, inner_header_field_4::INNER_RANDOM_STREAM_ID, &crs_id)?;
    write_inner_field(
        w,
        inner_header_field_4::INNER_RANDOM_STREAM_KEY,
        inner_stream_key,
    )?;
    for (data, protected) in binaries {
        let mut field = Vec::with_capacity(data.len() + 1);
        field.push(u8::from(*protected));
        field.extend_from_slice(data);
        write_inner_field(w, inner_header_field_4::BINARY, &field)?;
    }
    write_inner_field(w, inner_header_field_4::END_OF_HEADER, &[])?;
    Ok(())
}

fn collect_binaries(database: &Database) -> Vec<(Vec<u8>, bool)> {
    fn collect_entry(entry: &crate::model::entry::Entry, output: &mut Vec<(Vec<u8>, bool)>) {
        output.extend(
            entry
                .binaries
                .iter()
                .map(|binary| (binary.data.clone(), binary.is_protected)),
        );
        for history in &entry.history {
            collect_entry(history, output);
        }
    }

    fn collect_group(
        group: &crate::model::group::Group,
        database: &Database,
        output: &mut Vec<(Vec<u8>, bool)>,
    ) {
        for child_id in &group.child_group_ids {
            if let Some(child) = database.groups.get(child_id) {
                collect_group(child, database, output);
            }
        }
        for entry_id in &group.child_entry_ids {
            if let Some(entry) = database.entries.get(entry_id) {
                collect_entry(entry, output);
            }
        }
    }

    let mut output = Vec::new();
    if let Some(root) = database.root_group() {
        collect_group(root, database, &mut output);
    }
    output
}

fn write_header_field_4<W: Write>(w: &mut W, field_id: u8, data: &[u8]) -> DatabaseResult<()> {
    w.write_u8(field_id)?;
    let len = u32::try_from(data.len())
        .map_err(|_| DatabaseError::InvalidFormat("Outer header field is too large".into()))?;
    w.write_u32::<LittleEndian>(len)?;
    w.write_all(data)?;
    Ok(())
}

fn write_inner_field<W: Write>(w: &mut W, field_id: u8, data: &[u8]) -> DatabaseResult<()> {
    w.write_u8(field_id)?;
    let len = u32::try_from(data.len())
        .map_err(|_| DatabaseError::InvalidFormat("Inner header field is too large".into()))?;
    w.write_u32::<LittleEndian>(len)?;
    w.write_all(data)?;
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
        let mut db = Database::new(DatabaseVersion::KDBX4);
        let root_id = NodeId::from_uuid(Uuid::new_v4());
        let mut root = Group::new(root_id);
        root.title = "Root".to_string();

        let entry_id = NodeId::from_uuid(Uuid::new_v4());
        let mut entry = Entry::new(entry_id);
        entry.title = "KDBX4 Test".to_string();
        entry.password = crate::model::core::security::ProtectedString::new_protected("p@ssw0rd");
        entry.binaries.push(crate::model::entry::EntryBinary {
            name: "protected.bin".to_string(),
            data: vec![0, 1, 2, 255],
            is_protected: true,
        });

        root.add_child_entry(entry_id);
        db.entries.insert(entry_id, entry);
        db.groups.insert(root_id, root);
        db.root_group_id = Some(root_id);

        let key = CompositeKey::new().with_password(b"test_pass").unwrap();

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
        assert_eq!(e.binaries.len(), 1);
        assert_eq!(e.binaries[0].name, "protected.bin");
        assert_eq!(e.binaries[0].data, vec![0, 1, 2, 255]);
        assert!(e.binaries[0].is_protected);
    }
}
