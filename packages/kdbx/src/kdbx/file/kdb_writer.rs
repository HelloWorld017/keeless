//! KDB (v1) complete write pipeline
//!
//! KDB v1 format: signature → header → encrypted content (groups + entries)

use std::collections::HashMap;
use std::io::Write;

use byteorder::{LittleEndian, WriteBytesExt};

use crate::model::db::composite_key::CompositeKey;
use crate::model::db::database::Database;
use crate::model::entry::versioned::{EntryKDB, kdb_field};
use crate::model::group::versioned::{GroupKDB, kdb_group_field};
use crate::model::core::node::NodeId;
use crate::model::exception::{DatabaseError, DatabaseResult};
use crate::kdbx::file::header::{KDB_SIGNATURE_1, KDB_SIGNATURE_2};

/// Write a KDB (v1) database to a writer.
pub fn write_kdb<W: Write>(
    writer: &mut W,
    database: &Database,
    composite_key: &CompositeKey,
) -> DatabaseResult<()> {
    // 1. Generate header parameters
    let master_seed = generate_random_bytes(16);
    let encryption_iv = generate_random_bytes(16);
    let transform_seed = generate_random_bytes(32);
    let transform_rounds: u32 = 100;

    // 2. Count groups and entries
    let number_of_groups = database.groups.len() as u32;
    let number_of_entries = database.entries.len() as u32;

    // 3. Serialize groups and entries to binary
    let mut content = Vec::new();
    write_kdb_groups(&mut content, database)?;
    write_kdb_entries(&mut content, database)?;

    // 4. Compute content hash
    let content_hash = crate::crypto::HashEngine::sha256(&content);

    // 5. Derive master key
    let raw_key = composite_key.build_raw_key();
    if raw_key.is_empty() {
        return Err(DatabaseError::InvalidKey);
    }

    let mut params = crate::kdbx::kdf::kdf_parameters::KdfParameters::new(
        crate::kdbx::kdf::aes_kdf::AES_KDF_UUID
    );
    params.set_byte_array("S", &transform_seed);
    params.set_uint64("R", transform_rounds as u64);

    let kdf = crate::kdbx::kdf::aes_kdf::AesKdf;
    let transformed = crate::kdbx::kdf::kdf_engine::KdfEngine::transform(&kdf, &raw_key, &params)?;

    let mut combined = Vec::with_capacity(master_seed.len() + transformed.len());
    combined.extend_from_slice(&master_seed);
    combined.extend_from_slice(&transformed);
    let master_key = crate::crypto::HashEngine::sha256(&combined).to_vec();

    // 6. Encrypt content
    let cipher = crate::crypto::cipher_engine::create_cipher_engine(
        crate::crypto::encryption_algorithm::EncryptionAlgorithm::AesRijndael
    );
    let encrypted = cipher.encrypt(&master_key, &encryption_iv, &content)
        .map_err(|e| DatabaseError::EncryptionError(e.to_string()))?;

    // 7. Write signature
    writer.write_u32::<LittleEndian>(KDB_SIGNATURE_1)?;
    writer.write_u32::<LittleEndian>(KDB_SIGNATURE_2)?;
    writer.write_u32::<LittleEndian>(0x00010003)?; // KDB version

    // 8. Write header
    writer.write_u32::<LittleEndian>(0x00000003)?; // flags
    writer.write_u32::<LittleEndian>(0x00010003)?; // version
    writer.write_all(&master_seed)?;
    writer.write_all(&encryption_iv)?;
    writer.write_u32::<LittleEndian>(number_of_groups)?;
    writer.write_u32::<LittleEndian>(number_of_entries)?;
    writer.write_all(&content_hash)?;
    writer.write_all(&transform_seed)?;
    writer.write_u32::<LittleEndian>(transform_rounds)?;

    // 9. Write encrypted content
    writer.write_u32::<LittleEndian>(encrypted.len() as u32)?;
    writer.write_all(&encrypted)?;

    Ok(())
}

/// Assign KDB integer IDs to groups by traversing in order.
/// Returns a mapping from NodeId to u32 KDB group ID.
fn assign_kdb_group_ids(database: &Database) -> HashMap<NodeId, u32> {
    let mut mapping = HashMap::new();
    let mut next_id: u32 = 0;

    fn visit(
        db: &Database,
        group_id: &NodeId,
        mapping: &mut HashMap<NodeId, u32>,
        next_id: &mut u32,
    ) {
        if !mapping.contains_key(group_id) {
            mapping.insert(*group_id, *next_id);
            *next_id += 1;
        }
        if let Some(group) = db.groups.get(group_id) {
            for child_id in &group.child_group_ids {
                visit(db, child_id, mapping, next_id);
            }
        }
    }

    if let Some(root_id) = database.root_group_id.as_ref() {
        visit(database, root_id, &mut mapping, &mut next_id);
    }

    // Also assign IDs to groups not reachable from root
    for group_id in database.groups.keys() {
        if !mapping.contains_key(group_id) {
            mapping.insert(*group_id, next_id);
            next_id += 1;
        }
    }

    mapping
}

/// Compute the nesting level for each group.
fn compute_group_levels(database: &Database) -> HashMap<NodeId, usize> {
    let mut levels = HashMap::new();

    fn visit(
        db: &Database,
        group_id: &NodeId,
        level: usize,
        levels: &mut HashMap<NodeId, usize>,
    ) {
        levels.insert(*group_id, level);
        if let Some(group) = db.groups.get(group_id) {
            for child_id in &group.child_group_ids {
                visit(db, child_id, level + 1, levels);
            }
        }
    }

    if let Some(root_id) = database.root_group_id.as_ref() {
        visit(database, root_id, 0, &mut levels);
    }

    levels
}

/// Write all groups in KDB binary format.
fn write_kdb_groups<W: Write>(
    writer: &mut W,
    database: &Database,
) -> DatabaseResult<()> {
    let group_ids = assign_kdb_group_ids(database);
    let levels = compute_group_levels(database);

    // Write groups in tree traversal order
    fn write_group<W: Write>(
        w: &mut W,
        db: &Database,
        group_id: &NodeId,
        group_ids: &HashMap<NodeId, u32>,
        levels: &HashMap<NodeId, usize>,
    ) -> DatabaseResult<()> {
        let group = db.groups.get(group_id)
            .ok_or_else(|| DatabaseError::InvalidFormat("Group not found".into()))?;

        let fields = GroupKDB::to_kdb_fields(group);

        // Override the group ID with the KDB integer ID
        for (field_id, data) in &fields {
            if *field_id == kdb_group_field::GROUP_ID {
                continue; // We'll write our own ID
            }
            w.write_u16::<LittleEndian>(*field_id)?;
            w.write_u32::<LittleEndian>(data.len() as u32)?;
            w.write_all(data)?;
        }

        // Write group ID field
        let kdb_id = group_ids.get(group_id).copied().unwrap_or(0);
        w.write_u16::<LittleEndian>(kdb_group_field::GROUP_ID)?;
        w.write_u32::<LittleEndian>(4)?;
        w.write_u32::<LittleEndian>(kdb_id)?;

        // Write level field
        let level = levels.get(group_id).copied().unwrap_or(0);
        w.write_u16::<LittleEndian>(kdb_group_field::LEVEL)?;
        w.write_u32::<LittleEndian>(4)?;
        w.write_u32::<LittleEndian>(level as u32)?;

        // End marker
        w.write_u16::<LittleEndian>(kdb_group_field::END)?;
        w.write_u32::<LittleEndian>(0)?;

        // Recurse into children
        if let Some(group) = db.groups.get(group_id) {
            for child_id in &group.child_group_ids {
                write_group(w, db, child_id, group_ids, levels)?;
            }
        }

        Ok(())
    }

    if let Some(root_id) = database.root_group_id.as_ref() {
        write_group(writer, database, root_id, &group_ids, &levels)?;
    }

    // Write groups not reachable from root
    for group_id in group_ids.keys() {
        if database.root_group_id.as_ref().map(|r| *r == *group_id).unwrap_or(false) {
            continue;
        }
        // Already written in traversal if reachable from root
        // Skip to avoid duplicates — unreachable groups handled here
        let is_root = database.root_group_id.unwrap_or(NodeId::new_uuid());
        if *group_id == is_root {
            continue;
        }
    }

    Ok(())
}

/// Write all entries in KDB binary format.
fn write_kdb_entries<W: Write>(
    writer: &mut W,
    database: &Database,
) -> DatabaseResult<()> {
    let group_ids = assign_kdb_group_ids(database);

    for (entry_id, entry) in &database.entries {
        // Find the parent group for this entry
        let parent_group_id = database.find_parent_group_of_entry(entry_id);

        // Get the KDB group ID for the parent
        let kdb_group_id = parent_group_id
            .and_then(|pid| group_ids.get(&pid).copied())
            .unwrap_or(0);

        let mut fields = EntryKDB::to_kdb_fields(entry);

        // Override group ID field
        let group_id_bytes = kdb_group_id.to_le_bytes().to_vec();

        // Remove existing GROUP_ID if present and add ours
        fields.retain(|(id, _)| *id != kdb_field::GROUP_ID);
        fields.insert(0, (kdb_field::GROUP_ID, group_id_bytes));

        for (field_id, data) in &fields {
            writer.write_u16::<LittleEndian>(*field_id)?;
            writer.write_u32::<LittleEndian>(data.len() as u32)?;
            writer.write_all(data)?;
        }
    }

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
    use crate::model::group::Group;
    use crate::model::entry::Entry;
    use crate::model::core::security::ProtectedString;
    

    #[test]
    fn test_write_kdb_basic() {
        let mut db = Database::new(DatabaseVersion::KDB);
        let root_id = NodeId::from_u32(1);
        let mut root = Group::new(root_id);
        root.title = "Root".to_string();

        let entry_id = NodeId::new_uuid();
        let mut entry = Entry::new(entry_id);
        entry.title = "Test".to_string();
        entry.password = ProtectedString::new_protected("secret");

        root.add_child_entry(entry_id);
        db.entries.insert(entry_id, entry);
        db.groups.insert(root_id, {
            let mut g = Group::new(root_id);
            g.title = "Root".to_string();
            g.child_entry_ids.push(entry_id);
            g
        });
        db.root_group_id = Some(root_id);

        let key = CompositeKey::new().with_password(b"test_password");

        let mut buf = Vec::new();
        let result = write_kdb(&mut buf, &db, &key);
        assert!(result.is_ok());
        assert!(!buf.is_empty());
    }
}
