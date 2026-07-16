use super::*;
use crate::model::core::date::DateInstant;
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

fn entry_version(seed: &Entry, title: &str, modified: i64) -> Entry {
    let mut entry = seed.clone();
    entry.title = title.to_string();
    entry.last_modification_time = DateInstant::EpochMillis(modified);
    entry.history.clear();
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

#[test]
fn overwrite_preserves_both_histories_and_previous_target() {
    let entry_id = NodeId::new_uuid();
    let seed = Entry::new(entry_id);
    let shared = entry_version(&seed, "shared", 50);

    let mut target_entry = entry_version(&seed, "target", 200);
    target_entry.history = vec![shared.clone(), entry_version(&seed, "target-old", 100)];
    let mut source_entry = entry_version(&seed, "source", 300);
    source_entry.history = vec![shared, entry_version(&seed, "source-old", 100)];

    let mut target = Database::new(DatabaseVersion::KDBX4);
    target.entries.insert(entry_id, target_entry);
    let mut source = Database::new(DatabaseVersion::KDBX4);
    source.entries.insert(entry_id, source_entry);

    let result = DatabaseMerger::new(MergeStrategy::Overwrite).merge(&mut target, &source);
    let merged = &target.entries[&entry_id];

    assert_eq!(result.entries_modified, 1);
    assert_eq!(merged.title, "source");
    assert_eq!(
        merged
            .history
            .iter()
            .map(|entry| entry.title.as_str())
            .collect::<Vec<_>>(),
        vec!["shared", "source-old", "target-old", "target"]
    );
    assert!(merged
        .history
        .iter()
        .all(|entry| entry.id == entry_id && entry.history.is_empty()));

    let repeated = DatabaseMerger::new(MergeStrategy::Overwrite).merge(&mut target, &source);
    assert_eq!(repeated.entries_modified, 0);
    assert_eq!(target.entries[&entry_id].history.len(), 4);
}

#[test]
fn keep_existing_records_incoming_current_state() {
    let entry_id = NodeId::new_uuid();
    let seed = Entry::new(entry_id);
    let target_entry = entry_version(&seed, "target", 200);
    let mut source_entry = entry_version(&seed, "source", 300);
    source_entry.history = vec![entry_version(&seed, "source-old", 100)];

    let mut target = Database::new(DatabaseVersion::KDBX4);
    target.entries.insert(entry_id, target_entry);
    let mut source = Database::new(DatabaseVersion::KDBX4);
    source.entries.insert(entry_id, source_entry);

    let result = DatabaseMerger::new(MergeStrategy::KeepExisting).merge(&mut target, &source);
    let merged = &target.entries[&entry_id];

    assert_eq!(result.entries_modified, 1);
    assert_eq!(merged.title, "target");
    assert_eq!(
        merged
            .history
            .iter()
            .map(|entry| entry.title.as_str())
            .collect::<Vec<_>>(),
        vec!["source-old", "source"]
    );
}

#[test]
fn three_way_history_only_changes_merge_without_conflict() {
    let entry_id = NodeId::new_uuid();
    let seed = Entry::new(entry_id);
    let mut base_entry = entry_version(&seed, "current", 300);
    base_entry.history = vec![entry_version(&seed, "base-old", 50)];

    let mut target_entry = base_entry.clone();
    target_entry
        .history
        .push(entry_version(&seed, "target-old", 100));
    let mut source_entry = base_entry.clone();
    source_entry
        .history
        .push(entry_version(&seed, "source-old", 200));

    let mut base = Database::new(DatabaseVersion::KDBX4);
    base.entries.insert(entry_id, base_entry);
    let mut target = Database::new(DatabaseVersion::KDBX4);
    target.entries.insert(entry_id, target_entry);
    let mut source = Database::new(DatabaseVersion::KDBX4);
    source.entries.insert(entry_id, source_entry);

    let result =
        DatabaseMerger::new(MergeStrategy::Overwrite).merge_three_way(&mut target, &source, &base);
    let merged = &target.entries[&entry_id];

    assert!(result.conflicts.is_empty());
    assert_eq!(result.entries_modified, 1);
    assert_eq!(
        merged
            .history
            .iter()
            .map(|entry| entry.title.as_str())
            .collect::<Vec<_>>(),
        vec!["base-old", "target-old", "source-old"]
    );
}

#[test]
fn three_way_conflict_preserves_base_and_losing_current_state() {
    let entry_id = NodeId::new_uuid();
    let seed = Entry::new(entry_id);
    let base_entry = entry_version(&seed, "base", 100);
    let target_entry = entry_version(&seed, "target", 200);
    let source_entry = entry_version(&seed, "source", 300);

    let mut base = Database::new(DatabaseVersion::KDBX4);
    base.entries.insert(entry_id, base_entry);
    let mut target = Database::new(DatabaseVersion::KDBX4);
    target.entries.insert(entry_id, target_entry);
    let mut source = Database::new(DatabaseVersion::KDBX4);
    source.entries.insert(entry_id, source_entry);

    let result =
        DatabaseMerger::new(MergeStrategy::Overwrite).merge_three_way(&mut target, &source, &base);
    let merged = &target.entries[&entry_id];

    assert_eq!(result.conflicts.len(), 1);
    assert_eq!(merged.title, "source");
    assert_eq!(
        merged
            .history
            .iter()
            .map(|entry| entry.title.as_str())
            .collect::<Vec<_>>(),
        vec!["base", "target"]
    );
}

#[test]
fn keep_both_rewrites_duplicate_history_ids() {
    let entry_id = NodeId::new_uuid();
    let seed = Entry::new(entry_id);
    let mut target_entry = entry_version(&seed, "target", 200);
    target_entry.history = vec![entry_version(&seed, "target-old", 100)];
    let mut source_entry = entry_version(&seed, "source", 300);
    source_entry.history = vec![entry_version(&seed, "source-old", 150)];

    let mut target = Database::new(DatabaseVersion::KDBX4);
    target.entries.insert(entry_id, target_entry.clone());
    let mut source = Database::new(DatabaseVersion::KDBX4);
    source.entries.insert(entry_id, source_entry);

    DatabaseMerger::new(MergeStrategy::KeepBoth).merge(&mut target, &source);

    assert_eq!(target.entries[&entry_id], target_entry);
    let duplicate = target
        .entries
        .values()
        .find(|entry| entry.id != entry_id)
        .expect("duplicate exists");
    assert_eq!(duplicate.title, "source");
    assert!(duplicate
        .history
        .iter()
        .all(|history| history.id == duplicate.id));
}

#[test]
fn merge_does_not_truncate_history() {
    let entry_id = NodeId::new_uuid();
    let seed = Entry::new(entry_id);
    let mut target_entry = entry_version(&seed, "target", 20);
    target_entry.history = (0..12)
        .map(|version| entry_version(&seed, &format!("v{version}"), version))
        .collect();
    let source_entry = entry_version(&seed, "source", 30);

    let mut target = Database::new(DatabaseVersion::KDBX4);
    target.entries.insert(entry_id, target_entry);
    let mut source = Database::new(DatabaseVersion::KDBX4);
    source.entries.insert(entry_id, source_entry);

    DatabaseMerger::new(MergeStrategy::Overwrite).merge(&mut target, &source);

    assert_eq!(target.entries[&entry_id].history.len(), 13);
    assert_eq!(target.entries[&entry_id].history[12].title, "target");
}

#[test]
fn three_way_history_change_conflicts_with_deletion() {
    let entry_id = NodeId::new_uuid();
    let seed = Entry::new(entry_id);
    let base_entry = entry_version(&seed, "current", 200);
    let mut source_entry = base_entry.clone();
    source_entry.history = vec![entry_version(&seed, "old", 100)];

    let mut base = Database::new(DatabaseVersion::KDBX4);
    base.entries.insert(entry_id, base_entry);
    let mut target = Database::new(DatabaseVersion::KDBX4);
    target.deleted_objects.push(DeletedObject {
        id: entry_id,
        deletion_time: 300,
    });
    let mut source = Database::new(DatabaseVersion::KDBX4);
    source.entries.insert(entry_id, source_entry);

    let result =
        DatabaseMerger::new(MergeStrategy::Overwrite).merge_three_way(&mut target, &source, &base);

    assert_eq!(result.conflicts.len(), 1);
    assert_eq!(
        result.conflicts[0].conflict_type,
        ConflictType::EntryDeleteVsModify
    );
    assert_eq!(target.entries[&entry_id].history.len(), 1);
    assert!(!target
        .deleted_objects
        .iter()
        .any(|deleted| deleted.id == entry_id));
}
