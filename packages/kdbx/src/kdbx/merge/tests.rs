use super::*;
use crate::model::core::security::ProtectedString;
use crate::model::db::database::DatabaseVersion;
use crate::model::meta::DeletedObject;

fn database_with_root(root_id: NodeId) -> Database {
    let mut db = Database::new(DatabaseVersion::KDBX4);
    db.root_group_id = Some(root_id);
    db.groups.insert(root_id, Group::new(root_id));
    db
}

fn make_entry_with_title(id: NodeId, title: &str) -> Entry {
    let mut entry = Entry::new(id);
    entry.title = title.to_string();
    entry
}

fn make_entry_newer(id: NodeId, title: &str) -> Entry {
    let mut entry = Entry::new(id);
    entry.title = title.to_string();
    entry.last_modification_time = crate::model::core::date::DateInstant::EpochMillis(
        entry.last_modification_time.as_millis().unwrap_or(0) + 100_000,
    );
    entry
}

#[test]
fn test_merge_new_entry() {
    let mut target = Database::new(DatabaseVersion::KDBX4);
    let mut source = Database::new(DatabaseVersion::KDBX4);

    let entry_id = NodeId::new_uuid();
    let entry = make_entry_with_title(entry_id, "New Entry");
    source.entries.insert(entry_id, entry);

    let merger = DatabaseMerger::new(MergeStrategy::Overwrite);
    let result = merger.merge(&mut target, &source);

    assert_eq!(result.entries_added, 1);
    assert_eq!(target.entries.len(), 1);
}

#[test]
fn test_merge_keep_existing() {
    let mut target = Database::new(DatabaseVersion::KDBX4);
    let mut source = Database::new(DatabaseVersion::KDBX4);

    let entry_id = NodeId::new_uuid();
    let existing = make_entry_with_title(entry_id, "Original");
    target.entries.insert(entry_id, existing);

    let incoming = make_entry_with_title(entry_id, "Modified");
    source.entries.insert(entry_id, incoming);

    let merger = DatabaseMerger::new(MergeStrategy::KeepExisting);
    merger.merge(&mut target, &source);

    assert_eq!(target.entries.get(&entry_id).unwrap().title, "Original");
}

#[test]
fn test_merge_overwrite() {
    let mut target = Database::new(DatabaseVersion::KDBX4);
    let mut source = Database::new(DatabaseVersion::KDBX4);

    let entry_id = NodeId::new_uuid();
    target
        .entries
        .insert(entry_id, make_entry_with_title(entry_id, "Old"));
    source
        .entries
        .insert(entry_id, make_entry_newer(entry_id, "New"));

    let merger = DatabaseMerger::new(MergeStrategy::Overwrite);
    let result = merger.merge(&mut target, &source);

    assert_eq!(result.entries_modified, 1);
    assert_eq!(target.entries.get(&entry_id).unwrap().title, "New");
}

#[test]
fn test_merge_keep_both() {
    let mut target = Database::new(DatabaseVersion::KDBX4);
    let mut source = Database::new(DatabaseVersion::KDBX4);

    let entry_id = NodeId::new_uuid();
    target
        .entries
        .insert(entry_id, make_entry_with_title(entry_id, "Original"));
    source
        .entries
        .insert(entry_id, make_entry_newer(entry_id, "Modified"));

    let merger = DatabaseMerger::new(MergeStrategy::KeepBoth);
    merger.merge(&mut target, &source);

    assert_eq!(target.entries.len(), 2);
    assert!(target.entries.contains_key(&entry_id));
}

#[test]
fn test_three_way_merge_source_only_change() {
    let mut base = Database::new(DatabaseVersion::KDBX4);
    let mut target = Database::new(DatabaseVersion::KDBX4);
    let mut source = Database::new(DatabaseVersion::KDBX4);

    let entry_id = NodeId::new_uuid();
    let base_entry = make_entry_with_title(entry_id, "Base");
    base.entries.insert(entry_id, base_entry.clone());
    target.entries.insert(entry_id, base_entry.clone());
    let mut source_entry = base_entry;
    source_entry.title = "Source Modified".into();
    source_entry.last_modification_time =
        crate::model::core::date::DateInstant::EpochMillis(source_entry.last_modified() + 100_000);
    source.entries.insert(entry_id, source_entry);

    let merger = DatabaseMerger::new(MergeStrategy::Overwrite);
    let result = merger.merge_three_way(&mut target, &source, &base);

    assert_eq!(result.conflicts.len(), 0);
    assert_eq!(
        target.entries.get(&entry_id).unwrap().title,
        "Source Modified"
    );
}

#[test]
fn test_three_way_merge_no_change() {
    let mut base = Database::new(DatabaseVersion::KDBX4);
    let mut target = Database::new(DatabaseVersion::KDBX4);
    let mut source = Database::new(DatabaseVersion::KDBX4);

    let entry_id = NodeId::new_uuid();
    let entry = make_entry_with_title(entry_id, "Same");
    base.entries.insert(entry_id, entry.clone());
    target.entries.insert(entry_id, entry.clone());
    source.entries.insert(entry_id, entry);

    let merger = DatabaseMerger::new(MergeStrategy::Overwrite);
    let result = merger.merge_three_way(&mut target, &source, &base);

    assert_eq!(result.entries_modified, 0);
    assert_eq!(result.conflicts.len(), 0);
}

#[test]
fn test_three_way_merge_new_in_source() {
    let base = Database::new(DatabaseVersion::KDBX4);
    let mut target = Database::new(DatabaseVersion::KDBX4);
    let mut source = Database::new(DatabaseVersion::KDBX4);

    let new_id = NodeId::new_uuid();
    source
        .entries
        .insert(new_id, make_entry_with_title(new_id, "New Entry"));

    let merger = DatabaseMerger::new(MergeStrategy::Overwrite);
    let result = merger.merge_three_way(&mut target, &source, &base);

    assert_eq!(result.entries_added, 1);
    assert!(target.entries.contains_key(&new_id));
}

#[test]
fn test_entry_differs() {
    let id = NodeId::new_uuid();
    let a = make_entry_with_title(id, "A");
    let b = make_entry_with_title(id, "B");
    assert!(entry_differs(&a, &b));

    let c = make_entry_with_title(id, "Same");
    let d = make_entry_with_title(id, "Same");
    assert!(!entry_differs(&c, &d));
}

#[test]
fn three_way_password_only_change_conflicts() {
    let root_id = NodeId::new_uuid();
    let entry_id = NodeId::new_uuid();
    let mut base = database_with_root(root_id);
    let mut entry = Entry::new(entry_id);
    entry.password = ProtectedString::new_protected("base");
    base.add_entry(entry.clone(), &root_id);

    let mut target = database_with_root(root_id);
    let mut target_entry = entry.clone();
    target_entry.password = ProtectedString::new_protected("target");
    target.add_entry(target_entry, &root_id);

    let mut source = database_with_root(root_id);
    entry.password = ProtectedString::new_protected("source");
    source.add_entry(entry, &root_id);

    let result = DatabaseMerger::new(MergeStrategy::KeepExisting).merge_three_way(
        &mut target,
        &source,
        &base,
    );

    assert_eq!(result.conflicts.len(), 1);
    assert_eq!(target.entries[&entry_id].password.as_str(), "target");
    target.validate().unwrap();
}

#[test]
fn added_group_and_entry_are_attached_to_source_parent() {
    let root_id = NodeId::new_uuid();
    let group_id = NodeId::new_uuid();
    let entry_id = NodeId::new_uuid();
    let mut target = database_with_root(root_id);
    let mut source = database_with_root(root_id);
    source.add_group(Group::new(group_id), &root_id);
    source.add_entry(Entry::new(entry_id), &group_id);

    DatabaseMerger::new(MergeStrategy::Overwrite).merge(&mut target, &source);

    assert_eq!(target.find_parent_group_of_entry(&entry_id), Some(group_id));
    assert!(target.groups[&root_id].child_group_ids.contains(&group_id));
    target.validate().unwrap();
}

#[test]
fn keep_both_duplicate_is_attached() {
    let root_id = NodeId::new_uuid();
    let entry_id = NodeId::new_uuid();
    let mut target = database_with_root(root_id);
    let original = make_entry_with_title(entry_id, "target");
    target.add_entry(original, &root_id);
    let mut source = database_with_root(root_id);
    source.add_entry(make_entry_newer(entry_id, "source"), &root_id);

    DatabaseMerger::new(MergeStrategy::KeepBoth).merge(&mut target, &source);

    assert_eq!(target.entries.len(), 2);
    assert_eq!(target.groups[&root_id].child_entry_ids.len(), 2);
    target.validate().unwrap();
}

#[test]
fn source_deletion_uses_timestamp_and_cleans_parent_reference() {
    let root_id = NodeId::new_uuid();
    let entry_id = NodeId::new_uuid();
    let mut target = database_with_root(root_id);
    let mut entry = Entry::new(entry_id);
    entry.last_modification_time = crate::model::core::date::DateInstant::EpochMillis(100);
    target.add_entry(entry, &root_id);
    let mut source = database_with_root(root_id);
    source.deleted_objects.push(DeletedObject {
        id: entry_id,
        deletion_time: 99,
    });

    let old = DatabaseMerger::new(MergeStrategy::Overwrite).merge(&mut target, &source);
    assert!(target.entries.contains_key(&entry_id));
    assert_eq!(old.conflicts.len(), 1);

    source.deleted_objects[0].deletion_time = 101;
    let new = DatabaseMerger::new(MergeStrategy::Overwrite).merge(&mut target, &source);
    assert_eq!(new.entries_deleted, 1);
    assert!(!target.groups[&root_id].child_entry_ids.contains(&entry_id));
    target.validate().unwrap();
}

#[test]
fn three_way_group_content_keeps_target_only_and_takes_source_only_change() {
    let root_id = NodeId::new_uuid();
    let group_id = NodeId::new_uuid();
    let mut base = database_with_root(root_id);
    let mut group = Group::new(group_id);
    group.title = "base".into();
    base.add_group(group.clone(), &root_id);

    let mut target = database_with_root(root_id);
    let mut target_group = group.clone();
    target_group.title = "target".into();
    target.add_group(target_group, &root_id);
    let mut source = database_with_root(root_id);
    source.add_group(group.clone(), &root_id);

    DatabaseMerger::new(MergeStrategy::Overwrite).merge_three_way(&mut target, &source, &base);
    assert_eq!(target.groups[&group_id].title, "target");

    let mut target = database_with_root(root_id);
    target.add_group(group.clone(), &root_id);
    let mut source = database_with_root(root_id);
    group.title = "source".into();
    source.add_group(group, &root_id);

    DatabaseMerger::new(MergeStrategy::Overwrite).merge_three_way(&mut target, &source, &base);
    assert_eq!(target.groups[&group_id].title, "source");
    target.validate().unwrap();
}

#[test]
fn three_way_group_parent_keeps_target_only_and_takes_source_only_move() {
    let root_id = NodeId::new_uuid();
    let left_id = NodeId::new_uuid();
    let right_id = NodeId::new_uuid();
    let child_id = NodeId::new_uuid();
    let build = |child_parent: NodeId| {
        let mut db = database_with_root(root_id);
        db.add_group(Group::new(left_id), &root_id);
        db.add_group(Group::new(right_id), &root_id);
        db.add_group(Group::new(child_id), &child_parent);
        db
    };
    let base = build(left_id);
    let source = build(left_id);
    let mut target = build(right_id);

    DatabaseMerger::new(MergeStrategy::Overwrite).merge_three_way(&mut target, &source, &base);
    assert_eq!(group_parent(&target, &child_id), Some(right_id));
    target.validate().unwrap();

    let source = build(right_id);
    let mut target = build(left_id);
    DatabaseMerger::new(MergeStrategy::Overwrite).merge_three_way(&mut target, &source, &base);
    assert_eq!(group_parent(&target, &child_id), Some(right_id));
    target.validate().unwrap();
}
