//! KDB (v1) complete read pipeline
//!
//! KDB v1 format: signature → header → encrypted content → groups → entries

use std::collections::HashMap;
use std::io::Read;

use byteorder::{LittleEndian, ReadBytesExt};
use secure_types::{SecureArray, SecureBytes};
use zeroize::Zeroizing;

use crate::crypto::encryption_algorithm::EncryptionAlgorithm;
use crate::kdbx::file::header::KdbHeader;
use crate::model::core::node::NodeId;
use crate::model::db::composite_key::CompositeKey;
use crate::model::db::database::{Database, DatabaseVersion};
use crate::model::entry::versioned::{kdb_field, EntryKDB};
use crate::model::exception::{DatabaseError, DatabaseResult};
use crate::model::group::versioned::{kdb_group_field, GroupKDB};

/// Read a KDB (v1) database from a reader.
/// The caller should have already consumed the 12-byte signature/version.
pub fn read_kdb<R: Read>(reader: &mut R, composite_key: &CompositeKey) -> DatabaseResult<Database> {
    // 1. Read header
    let header = read_kdb_header(reader)?;

    // 2. Derive master key
    let master_key = derive_kdb_master_key(composite_key, &header)?;

    // 3. Read encrypted content
    let encrypted_size = reader.read_u32::<LittleEndian>()? as usize;
    if encrypted_size == 0 || encrypted_size > 100 * 1024 * 1024 {
        return Err(DatabaseError::InvalidFormat(
            "Invalid encrypted content size".into(),
        ));
    }
    let mut encrypted = vec![0u8; encrypted_size];
    reader.read_exact(&mut encrypted)?;

    // 4. Decrypt content
    let cipher =
        crate::crypto::cipher_engine::create_cipher_engine(EncryptionAlgorithm::AesRijndael);
    let decrypted = Zeroizing::new(master_key.unlock(|key| {
        cipher
            .decrypt(key, &header.encryption_iv, &encrypted)
            .map_err(DatabaseError::from_decryption_error)
    })?);

    // 5. Verify content hash
    let content_hash = crate::crypto::HashEngine::sha256(&decrypted);
    if content_hash[..] != header.content_hash[..] {
        return Err(DatabaseError::IntegrityError(
            "KDB content hash mismatch — file may be corrupted or password is wrong".into(),
        ));
    }

    // 6. Parse groups and entries from decrypted content
    let mut cursor = std::io::Cursor::new(&decrypted);
    let groups = read_kdb_groups(&mut cursor, header.number_of_groups)?;
    let entries = read_kdb_entries(&mut cursor, header.number_of_entries)?;

    // 7. Build database
    let mut db = Database::new(DatabaseVersion::KDB);

    // Build group hierarchy
    // KDB groups are flat with level numbers to indicate nesting
    let mut group_stack: Vec<(NodeId, usize)> = Vec::new(); // (group_id, level)

    for (group_fields, _level) in &groups {
        let group = GroupKDB::from_kdb_fields(group_fields);
        let group_id = group.id;

        // Find parent based on level
        let level = *_level;
        // Pop stack until we find a group with level < current
        while group_stack
            .last()
            .map(|(_, l)| *l >= level)
            .unwrap_or(false)
        {
            group_stack.pop();
        }

        // Add to database
        db.groups.insert(group_id, group);

        // Set parent-child relationship
        if let Some((parent_id, _)) = group_stack.last() {
            if let Some(parent) = db.groups.get_mut(parent_id) {
                parent.add_child_group(group_id);
            }
        } else {
            // Root group — already inserted into db.groups above; just record its id.
            db.root_group_id = Some(group_id);
        }

        group_stack.push((group_id, level));
    }

    // Add entries to their groups
    for entry_fields in &entries {
        let group_id_u32 = entry_fields
            .get(&kdb_field::GROUP_ID)
            .and_then(|v| {
                v.get(..4)
                    .map(|s| u32::from_le_bytes(s.try_into().unwrap_or([0; 4])))
            })
            .unwrap_or(0);
        let group_id = NodeId::from_u32(group_id_u32);

        let entry = EntryKDB::from_kdb_fields(NodeId::new_uuid(), entry_fields);
        let entry_id = entry.id;

        if let Some(group) = db.groups.get_mut(&group_id) {
            group.add_child_entry(entry_id);
        }
        db.entries.insert(entry_id, entry);
    }

    Ok(db)
}

/// Read KDB header (after signature bytes).
fn read_kdb_header<R: Read>(reader: &mut R) -> DatabaseResult<KdbHeader> {
    let flags = reader.read_u32::<LittleEndian>()?;
    let version = reader.read_u32::<LittleEndian>()?;

    let mut master_seed = vec![0u8; 16];
    reader.read_exact(&mut master_seed)?;

    let mut encryption_iv = vec![0u8; 16];
    reader.read_exact(&mut encryption_iv)?;

    let number_of_groups = reader.read_u32::<LittleEndian>()?;
    let number_of_entries = reader.read_u32::<LittleEndian>()?;

    let mut content_hash = [0u8; 32];
    reader.read_exact(&mut content_hash)?;

    let mut transform_seed = vec![0u8; 32];
    reader.read_exact(&mut transform_seed)?;

    let transform_rounds = reader.read_u32::<LittleEndian>()?;

    Ok(KdbHeader {
        flags,
        version,
        master_seed,
        encryption_iv,
        number_of_groups,
        number_of_entries,
        content_hash,
        transform_seed,
        transform_rounds,
    })
}

/// Derive the master key for KDB format.
fn derive_kdb_master_key(
    composite_key: &CompositeKey,
    header: &KdbHeader,
) -> DatabaseResult<SecureArray<32>> {
    let raw_key = composite_key.build_raw_key()?;

    // KDB uses AES-KDF with the transform seed
    let mut params = crate::kdbx::kdf::kdf_parameters::KdfParameters::new(
        crate::kdbx::kdf::aes_kdf::AES_KDF_UUID,
    );
    params.set_byte_array("S", &header.transform_seed);
    params.set_uint64("R", header.transform_rounds as u64);

    let kdf = crate::kdbx::kdf::aes_kdf::AesKdf;
    let transformed =
        SecureBytes::from_vec(raw_key.unlock(|key| {
            crate::kdbx::kdf::kdf_engine::KdfEngine::transform(&kdf, key, &params)
        })?)?;

    // Combine with master seed
    let mut combined = Zeroizing::new(Vec::with_capacity(
        header.master_seed.len() + transformed.len(),
    ));
    combined.extend_from_slice(&header.master_seed);
    transformed.unlock_slice(|value| combined.extend_from_slice(value));
    let mut master_key = crate::crypto::HashEngine::sha256(&combined);
    Ok(SecureArray::from_array_mut(&mut master_key)?)
}

type KdbGroupList = Vec<(HashMap<u16, Vec<u8>>, usize)>;

/// Read KDB groups from decrypted content.
fn read_kdb_groups<R: Read>(reader: &mut R, count: u32) -> DatabaseResult<KdbGroupList> {
    let mut groups = Vec::with_capacity(count as usize);

    for _ in 0..count {
        let mut fields = HashMap::new();
        let mut level = 0usize;

        loop {
            let field_id = reader.read_u16::<LittleEndian>()?;
            if field_id == kdb_group_field::END {
                // Skip the 4-byte size field for END
                let _size = reader.read_u32::<LittleEndian>()?;
                break;
            }

            let field_size = reader.read_u32::<LittleEndian>()? as usize;
            if field_size > 0 {
                let mut data = vec![0u8; field_size];
                reader.read_exact(&mut data)?;

                if field_id == kdb_group_field::LEVEL {
                    if data.len() >= 4 {
                        level = u32::from_le_bytes(data[..4].try_into().unwrap_or([0; 4])) as usize;
                    }
                } else {
                    fields.insert(field_id, data);
                }
            }
        }

        groups.push((fields, level));
    }

    Ok(groups)
}

/// Read KDB entries from decrypted content.
fn read_kdb_entries<R: Read>(
    reader: &mut R,
    count: u32,
) -> DatabaseResult<Vec<HashMap<u16, Vec<u8>>>> {
    let mut entries = Vec::with_capacity(count as usize);

    for _ in 0..count {
        let mut fields = HashMap::new();

        loop {
            let field_id = reader.read_u16::<LittleEndian>()?;
            if field_id == kdb_field::END {
                let _size = reader.read_u32::<LittleEndian>()?;
                break;
            }

            let field_size = reader.read_u32::<LittleEndian>()? as usize;
            if field_size > 0 {
                let mut data = vec![0u8; field_size];
                reader.read_exact(&mut data)?;
                fields.insert(field_id, data);
            }
        }

        entries.push(fields);
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kdb_header_reading() {
        let mut data = Vec::new();
        data.extend_from_slice(&0x00000003u32.to_le_bytes()); // flags
        data.extend_from_slice(&0x00010003u32.to_le_bytes()); // version
        data.extend_from_slice(&[0u8; 16]); // master seed
        data.extend_from_slice(&[0u8; 16]); // encryption IV
        data.extend_from_slice(&2u32.to_le_bytes()); // number of groups
        data.extend_from_slice(&1u32.to_le_bytes()); // number of entries
        data.extend_from_slice(&[0u8; 32]); // content hash
        data.extend_from_slice(&[0x42u8; 32]); // transform seed
        data.extend_from_slice(&6000u32.to_le_bytes()); // transform rounds

        let mut cursor = std::io::Cursor::new(data);
        let header = read_kdb_header(&mut cursor).unwrap();

        assert_eq!(header.number_of_groups, 2);
        assert_eq!(header.number_of_entries, 1);
        assert_eq!(header.transform_rounds, 6000);
    }
}
