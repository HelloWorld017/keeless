use super::*;
use crate::model::core::security::ProtectedString;
use crate::model::exception::DatabaseError;

fn make_test_db() -> Database {
    let mut db = Database::new(DatabaseVersion::KDBX4);
    let root_id = NodeId::new_uuid();
    let root = Group::new(root_id);
    db.groups.insert(root_id, root);
    db.root_group_id = Some(root_id);
    db
}

#[test]
fn test_database_creation() {
    let db = Database::new(DatabaseVersion::KDBX4);
    assert!(!db.loaded);
    assert_eq!(db.entry_count(), 0);
    assert_eq!(db.group_count(), 0);
    assert_eq!(db.encryption_algorithm, EncryptionAlgorithm::AesRijndael);
}

#[test]
fn test_validate_rejects_dangling_duplicate_and_cyclic_graphs() {
    let mut dangling = make_test_db();
    dangling
        .root_group_mut()
        .unwrap()
        .child_entry_ids
        .push(NodeId::new_uuid());
    assert!(matches!(
        dangling.validate(),
        Err(DatabaseError::InvalidFormat(_))
    ));

    let mut duplicate = make_test_db();
    let entry_id = NodeId::new_uuid();
    duplicate.entries.insert(entry_id, Entry::new(entry_id));
    duplicate
        .root_group_mut()
        .unwrap()
        .child_entry_ids
        .extend([entry_id, entry_id]);
    assert!(matches!(
        duplicate.validate(),
        Err(DatabaseError::InvalidFormat(_))
    ));

    let mut cyclic = make_test_db();
    let root = cyclic.root_group_id.unwrap();
    let child = NodeId::new_uuid();
    let mut child_group = Group::new(child);
    child_group.child_group_ids.push(root);
    cyclic.groups.insert(child, child_group);
    cyclic
        .groups
        .get_mut(&root)
        .unwrap()
        .child_group_ids
        .push(child);
    assert!(matches!(
        cyclic.validate(),
        Err(DatabaseError::InvalidFormat(_))
    ));
}

#[test]
fn test_validate_rejects_unreachable_and_key_mismatch() {
    let mut unreachable = make_test_db();
    let group_id = NodeId::new_uuid();
    unreachable.groups.insert(group_id, Group::new(group_id));
    assert!(matches!(
        unreachable.validate(),
        Err(DatabaseError::InvalidFormat(_))
    ));

    let mut mismatch = make_test_db();
    let root = mismatch.root_group_id.unwrap();
    mismatch.groups.get_mut(&root).unwrap().id = NodeId::new_uuid();
    assert!(matches!(
        mismatch.validate(),
        Err(DatabaseError::InvalidFormat(_))
    ));
}

#[test]
fn test_add_entry() {
    let mut db = make_test_db();
    let root_id = db.root_group_id.unwrap();

    let entry_id = NodeId::new_uuid();
    let mut entry = Entry::new(entry_id);
    entry.title = "Test Entry".to_string();
    entry.password = ProtectedString::new_protected("secret");

    assert!(db.add_entry(entry, &root_id));
    assert_eq!(db.entry_count(), 1);
    assert_eq!(db.get_entry(&entry_id).unwrap().title, "Test Entry");
}

#[test]
fn test_add_entry_to_nonexistent_group() {
    let mut db = make_test_db();
    let fake_group = NodeId::new_uuid();
    let entry = Entry::new(NodeId::new_uuid());
    assert!(!db.add_entry(entry, &fake_group));
}

#[test]
fn test_remove_entry_permanent() {
    let mut db = make_test_db();
    let root_id = db.root_group_id.unwrap();

    let entry_id = NodeId::new_uuid();
    let entry = Entry::new(entry_id);
    db.add_entry(entry, &root_id);

    let removed = db.remove_entry(&entry_id, false).unwrap();
    assert_eq!(removed.id, entry_id);
    assert_eq!(db.entry_count(), 0);
    assert!(db
        .deleted_objects
        .iter()
        .any(|deleted| deleted.id == entry_id));
}

#[test]
fn test_remove_entry_to_recycle_bin() {
    let mut db = make_test_db();
    let root_id = db.root_group_id.unwrap();

    let entry_id = NodeId::new_uuid();
    let entry = Entry::new(entry_id);
    db.add_entry(entry, &root_id);

    let recycle_id = db.create_recycle_bin();
    assert!(db.recycle_bin_uuid.is_some());

    let removed = db.remove_entry(&entry_id, true).unwrap();
    assert_eq!(removed.id, entry_id);
    assert_eq!(db.entry_count(), 1);

    let recycle_entries = db.get_entries_in_group(&recycle_id);
    assert_eq!(recycle_entries.len(), 1);
}

#[test]
fn test_add_group() {
    let mut db = make_test_db();
    let root_id = db.root_group_id.unwrap();

    let group_id = NodeId::new_uuid();
    let mut group = Group::new(group_id);
    group.title = "Subgroup".to_string();

    assert!(db.add_group(group, &root_id));
    assert_eq!(db.group_count(), 2);
}

#[test]
fn test_reposition_group_changes_parent_and_order() {
    let mut db = make_test_db();
    let root_id = db.root_group_id.unwrap();
    let first_id = NodeId::new_uuid();
    let second_id = NodeId::new_uuid();
    let nested_id = NodeId::new_uuid();

    db.add_group(Group::new(first_id), &root_id);
    db.add_group(Group::new(second_id), &root_id);
    db.add_group(Group::new(nested_id), &first_id);
    db.get_group_mut(&nested_id).unwrap().location_changed = DateInstant::EpochMillis(0);

    assert!(db.reposition_group(&second_id, &root_id, 0));
    assert_eq!(
        db.get_group(&root_id).unwrap().child_group_ids,
        vec![second_id, first_id]
    );

    assert!(db.reposition_group(&nested_id, &root_id, 1));
    assert!(db.get_group(&first_id).unwrap().child_group_ids.is_empty());
    assert_eq!(
        db.get_group(&root_id).unwrap().child_group_ids,
        vec![second_id, nested_id, first_id]
    );
    assert!(db
        .get_group(&nested_id)
        .unwrap()
        .location_changed
        .as_millis()
        .is_some_and(|value| value > 0));
    assert!(db.data_modified);
}

#[test]
fn test_reposition_group_rejects_invalid_hierarchy_changes() {
    let mut db = make_test_db();
    let root_id = db.root_group_id.unwrap();
    let parent_id = NodeId::new_uuid();
    let child_id = NodeId::new_uuid();
    db.add_group(Group::new(parent_id), &root_id);
    db.add_group(Group::new(child_id), &parent_id);
    let recycle_id = db.create_recycle_bin();

    assert!(!db.reposition_group(&root_id, &parent_id, 0));
    assert!(!db.reposition_group(&parent_id, &child_id, 0));
    assert!(!db.reposition_group(&parent_id, &parent_id, 0));
    assert!(!db.reposition_group(&parent_id, &recycle_id, 0));
    assert!(!db.reposition_group(&recycle_id, &root_id, 0));
    assert!(!db.reposition_group(&parent_id, &root_id, 3));
    assert_eq!(
        db.get_group(&root_id).unwrap().child_group_ids,
        vec![parent_id, recycle_id]
    );
    assert_eq!(
        db.get_group(&parent_id).unwrap().child_group_ids,
        vec![child_id]
    );
}

#[test]
fn test_remove_group() {
    let mut db = make_test_db();
    let root_id = db.root_group_id.unwrap();

    let group_id = NodeId::new_uuid();
    let group = Group::new(group_id);
    db.add_group(group, &root_id);

    let removed = db.remove_group(&group_id, false).unwrap();
    assert_eq!(removed.id, group_id);
    assert_eq!(db.group_count(), 1);
}

#[test]
fn test_cannot_remove_root_group() {
    let mut db = make_test_db();
    let root_id = db.root_group_id.unwrap();
    assert!(db.remove_group(&root_id, false).is_none());
}

#[test]
fn test_recycle_bin_creation() {
    let mut db = make_test_db();
    assert!(db.recycle_bin_uuid.is_none());

    let recycle_id = db.create_recycle_bin();
    assert!(db.recycle_bin_uuid.is_some());
    assert_eq!(db.get_group(&recycle_id).unwrap().title, "Recycle Bin");
}

#[test]
fn test_empty_recycle_bin() {
    let mut db = make_test_db();
    let root_id = db.root_group_id.unwrap();

    let entry_id = NodeId::new_uuid();
    let entry = Entry::new(entry_id);
    db.add_entry(entry, &root_id);

    let group_id = NodeId::new_uuid();
    let group = Group::new(group_id);
    db.add_group(group, &root_id);

    db.create_recycle_bin();
    db.remove_entry(&entry_id, true);
    db.remove_group(&group_id, true);

    assert_eq!(db.entry_count(), 1);
    assert_eq!(db.group_count(), 3);

    db.empty_recycle_bin();

    assert_eq!(db.entry_count(), 0);
    assert_eq!(db.group_count(), 2);
}

#[test]
fn test_get_all_entries_recursive() {
    let mut db = make_test_db();
    let root_id = db.root_group_id.unwrap();

    let sub_id = NodeId::new_uuid();
    let mut sub = Group::new(sub_id);
    sub.title = "Sub".to_string();
    db.add_group(sub, &root_id);

    db.add_entry(Entry::new(NodeId::new_uuid()), &root_id);
    db.add_entry(Entry::new(NodeId::new_uuid()), &sub_id);

    let all = db.get_all_entries_in_group(&root_id);
    assert_eq!(all.len(), 2);

    let root_only = db.get_entries_in_group(&root_id);
    assert_eq!(root_only.len(), 1);
}

#[test]
fn test_find_parent_group() {
    let mut db = make_test_db();
    let root_id = db.root_group_id.unwrap();

    let entry_id = NodeId::new_uuid();
    let entry = Entry::new(entry_id);
    db.add_entry(entry, &root_id);

    let found = db.find_parent_group_of_entry(&entry_id);
    assert_eq!(found, Some(root_id));
}
