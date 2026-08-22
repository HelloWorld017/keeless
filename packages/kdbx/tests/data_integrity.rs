//! Data-integrity guarantees across merges and KDBX4 round trips.

use std::io::Cursor;

use keeless_kdbx::kdbx::kdf::argon2_kdf::ARGON2ID_UUID;
use keeless_kdbx::kdbx::kdf::create_kdf;
use keeless_kdbx::{
    open_database, save_database, CompositeCredentials, Database, DatabaseMerger, DatabaseVersion,
    DateInstant, Entry, EntryBinary, Group, IconImageCustom, MergeStrategy, NodeId,
};
use uuid::Uuid;

const PASSWORD: &[u8] = b"data-integrity-test";

fn database_with_root(root_id: NodeId) -> Database {
    let mut database = Database::new(DatabaseVersion::KDBX4);
    let mut root = Group::new(root_id);
    root.title = "Root".to_string();
    database.groups.insert(root_id, root);
    database.root_group_id = Some(root_id);
    database
}

fn round_trip(mut database: Database) -> Database {
    if database.kdf_parameters.is_none() {
        let kdf = create_kdf(&ARGON2ID_UUID).expect("Argon2id KDF should be available");
        let mut parameters = kdf.default_parameters();
        parameters.set_uint64("M", 64 * 1024);
        parameters.set_uint64("I", 1);
        kdf.randomize(&mut parameters).unwrap();
        database.kdf_parameters = Some(parameters);
    }

    let credentials = CompositeCredentials::new().with_password(PASSWORD).unwrap();
    let key = credentials
        .derive_key(database.kdf_parameters.as_ref().unwrap())
        .unwrap();
    let mut bytes = Vec::new();
    save_database(&mut bytes, &database, &key).expect("database should be saved");
    open_database(Cursor::new(bytes), &credentials)
        .expect("database should be reopened")
        .database
}

fn entry_with_binary(id: NodeId, title: &str, data: &[u8], modified: i64) -> Entry {
    let mut entry = Entry::new(id);
    entry.set_title(title);
    entry.last_modification_time = DateInstant::EpochMillis(modified);
    entry.binaries.push(EntryBinary {
        name: "document.bin".to_string(),
        data: data.to_vec(),
        is_protected: true,
    });
    entry
}

fn child_titles(database: &Database) -> (Vec<String>, Vec<String>) {
    let root = database.root_group().expect("root group should exist");
    let groups = root
        .child_group_ids
        .iter()
        .map(|id| database.groups[id].title.clone())
        .collect();
    let entries = root
        .child_entry_ids
        .iter()
        .map(|id| database.entries[id].title().as_str().to_string())
        .collect();
    (groups, entries)
}

#[test]
fn merge_preserves_entry_binaries_custom_icons_and_history() {
    let root_id = NodeId::new_uuid();
    let entry_id = NodeId::new_uuid();
    let icon_id = Uuid::new_v4();

    let mut target = database_with_root(root_id);
    target.add_entry(
        entry_with_binary(entry_id, "Target", b"target attachment", 100),
        &root_id,
    );

    let mut source = database_with_root(root_id);
    let mut source_entry = entry_with_binary(entry_id, "Source", b"source attachment", 200);
    source_entry.custom_icon_uuid = Some(icon_id);
    source.add_entry(source_entry, &root_id);
    source.custom_icons.insert(
        icon_id,
        IconImageCustom::new(icon_id, b"custom icon".to_vec()),
    );

    DatabaseMerger::new(MergeStrategy::Overwrite).merge(&mut target, &source);

    let merged = &target.entries[&entry_id];
    assert_eq!(merged.binaries[0].data, b"source attachment");
    assert_eq!(merged.custom_icon_uuid, Some(icon_id));
    assert_eq!(target.custom_icons[&icon_id].data, b"custom icon");
    assert!(merged
        .history
        .iter()
        .any(|entry| entry.binaries[0].data == b"target attachment"));

    let reopened = round_trip(target);
    let merged = &reopened.entries[&entry_id];
    assert_eq!(merged.binaries[0].data, b"source attachment");
    assert_eq!(merged.custom_icon_uuid, Some(icon_id));
    assert_eq!(reopened.custom_icons[&icon_id].data, b"custom icon");
    assert!(merged
        .history
        .iter()
        .any(|entry| entry.binaries[0].data == b"target attachment"));
}

#[test]
fn three_way_merge_preserves_updated_binary_and_custom_icon() {
    let root_id = NodeId::new_uuid();
    let entry_id = NodeId::new_uuid();
    let icon_id = Uuid::new_v4();
    let base_entry = entry_with_binary(entry_id, "Base", b"base attachment", 100);

    let mut base = database_with_root(root_id);
    base.add_entry(base_entry.clone(), &root_id);

    let mut target = database_with_root(root_id);
    target.add_entry(base_entry.clone(), &root_id);

    let mut source = database_with_root(root_id);
    let mut source_entry = base_entry;
    source_entry.set_title("Source");
    source_entry.last_modification_time = DateInstant::EpochMillis(200);
    source_entry.binaries[0].data = b"source attachment".to_vec();
    source_entry.custom_icon_uuid = Some(icon_id);
    source.add_entry(source_entry, &root_id);
    source.custom_icons.insert(
        icon_id,
        IconImageCustom::new(icon_id, b"custom icon".to_vec()),
    );

    DatabaseMerger::new(MergeStrategy::Overwrite).merge_three_way(&mut target, &source, &base);

    let merged = &target.entries[&entry_id];
    assert_eq!(merged.binaries[0].data, b"source attachment");
    assert_eq!(merged.custom_icon_uuid, Some(icon_id));
    assert_eq!(target.custom_icons[&icon_id].data, b"custom icon");
}

#[test]
fn child_order_survives_round_trip_and_deletion() {
    let root_id = NodeId::new_uuid();
    let mut database = database_with_root(root_id);
    let mut group_ids = Vec::new();
    let mut entry_ids = Vec::new();
    let group_titles: Vec<_> = (0..20).map(|index| format!("Group_{index}")).collect();
    let entry_titles: Vec<_> = (0..20).map(|index| format!("Entry_{index}")).collect();

    for title in &group_titles {
        let id = NodeId::new_uuid();
        let mut group = Group::new(id);
        group.title = title.clone();
        assert!(database.add_group(group, &root_id));
        group_ids.push(id);
    }
    for title in &entry_titles {
        let id = NodeId::new_uuid();
        let mut entry = Entry::new(id);
        entry.set_title(title.clone());
        assert!(database.add_entry(entry, &root_id));
        entry_ids.push(id);
    }

    let mut reopened = round_trip(database);
    assert_eq!(
        child_titles(&reopened),
        (group_titles.clone(), entry_titles.clone())
    );

    let deleted_group_indexes = [0, 8, 19];
    let deleted_entry_indexes = [0, 7, 19];
    for index in deleted_group_indexes {
        reopened
            .remove_group(&group_ids[index], false)
            .expect("group should be removed");
    }
    for index in deleted_entry_indexes {
        reopened
            .remove_entry(&entry_ids[index], false)
            .expect("entry should be removed");
    }

    let expected_groups = group_titles
        .into_iter()
        .enumerate()
        .filter_map(|(index, title)| (!deleted_group_indexes.contains(&index)).then_some(title))
        .collect();
    let expected_entries = entry_titles
        .into_iter()
        .enumerate()
        .filter_map(|(index, title)| (!deleted_entry_indexes.contains(&index)).then_some(title))
        .collect();

    let reopened = round_trip(reopened);
    assert_eq!(child_titles(&reopened), (expected_groups, expected_entries));
}

#[test]
fn historical_binary_survives_removal_from_current_entry() {
    let root_id = NodeId::new_uuid();
    let entry_id = NodeId::new_uuid();
    let mut database = database_with_root(root_id);
    let mut entry = entry_with_binary(entry_id, "Entry", b"historical attachment", 100);

    entry.push_history();
    entry.binaries.clear();
    database.add_entry(entry, &root_id);

    let reopened = round_trip(database);
    let entry = &reopened.entries[&entry_id];
    assert!(entry.binaries.is_empty());
    assert_eq!(entry.history.len(), 1);
    assert_eq!(entry.history[0].binaries.len(), 1);
    assert_eq!(entry.history[0].binaries[0].data, b"historical attachment");
    assert!(entry.history[0].binaries[0].is_protected);
}
