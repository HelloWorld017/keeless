use keeless_kdbx::{
    open_database, save_database, ChangeTracker, CompositeKey, Database, DatabaseError,
    DatabaseVersion, DateInstant, Entry, EntryFieldId, EntryFieldUpdate, Group, NodeId,
    ProtectedString,
};

fn loaded_database() -> (Database, CompositeKey, NodeId) {
    let key = CompositeKey::new().with_password(b"test").unwrap();
    let mut database = Database::new(DatabaseVersion::KDBX4);
    let root_id = NodeId::new_uuid();
    database.groups.insert(root_id, Group::new(root_id));
    database.root_group_id = Some(root_id);
    let entry_id = NodeId::new_uuid();
    let mut entry = Entry::new(entry_id);
    entry.set_title("Old");
    entry.set_username("user");
    entry.set_password(ProtectedString::new_protected("password"));
    entry.set_url("https://old.test");
    entry.set_notes(ProtectedString::new_protected("notes"));
    entry.last_modification_time = DateInstant::EpochMillis(123);
    entry.add_custom_field("Duplicate", ProtectedString::new_protected("first"));
    entry.add_custom_field("Duplicate", ProtectedString::new_protected("second"));
    entry.add_custom_field("Remove", ProtectedString::new_plain("remove"));
    database.add_entry(entry, &root_id);
    let mut bytes = Vec::new();
    save_database(&mut bytes, &database, &key).unwrap();
    (
        open_database(bytes.as_slice(), &key).unwrap(),
        key,
        entry_id,
    )
}

fn source_field_id(entry: &Entry, name: &str, occurrence: usize) -> EntryFieldId {
    entry
        .fields()
        .filter(|(_, field)| field.name() == name)
        .nth(occurrence)
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("missing occurrence {occurrence} of field {name}"))
}

fn standard_fields(entry: &Entry) -> Vec<EntryFieldUpdate> {
    vec![
        EntryFieldUpdate {
            field_id: Some(source_field_id(entry, "Title", 0)),
            name: "Title".into(),
            value: Some("New".into()),
            is_protected: false,
        },
        EntryFieldUpdate {
            field_id: Some(source_field_id(entry, "UserName", 0)),
            name: "UserName".into(),
            value: Some("user".into()),
            is_protected: false,
        },
        EntryFieldUpdate {
            field_id: Some(source_field_id(entry, "Password", 0)),
            name: "Password".into(),
            value: None,
            is_protected: true,
        },
        EntryFieldUpdate {
            field_id: Some(source_field_id(entry, "URL", 0)),
            name: "URL".into(),
            value: Some("https://old.test".into()),
            is_protected: false,
        },
        EntryFieldUpdate {
            field_id: Some(source_field_id(entry, "Notes", 0)),
            name: "Notes".into(),
            value: None,
            is_protected: true,
        },
    ]
}

#[test]
fn update_reorders_renames_adds_and_deletes_without_confusing_duplicate_names() {
    let (mut database, key, entry_id) = loaded_database();
    let old_timestamp = database
        .get_entry(&entry_id)
        .unwrap()
        .last_modification_time;
    let source = database.get_entry(&entry_id).unwrap();
    let mut fields = standard_fields(source);
    fields.extend([
        EntryFieldUpdate {
            field_id: Some(source_field_id(source, "Duplicate", 1)),
            name: "Renamed".into(),
            value: None,
            is_protected: true,
        },
        EntryFieldUpdate {
            field_id: Some(source_field_id(source, "Duplicate", 0)),
            name: "Duplicate".into(),
            value: None,
            is_protected: true,
        },
        EntryFieldUpdate {
            field_id: None,
            name: "Added".into(),
            value: Some("added".into()),
            is_protected: true,
        },
    ]);

    assert!(database
        .update_entry_fields(&key, &entry_id, &fields)
        .unwrap());
    let entry = database.get_entry(&entry_id).unwrap();
    assert_eq!(
        entry
            .custom_fields()
            .map(|(_, field)| field.name())
            .collect::<Vec<_>>(),
        ["Renamed", "Duplicate", "Added"]
    );
    assert_eq!(entry.history.len(), 1);
    assert_eq!(entry.history[0].last_modification_time, old_timestamp);
    assert_ne!(entry.last_modification_time, old_timestamp);
    assert!(database.data_modified);
    let custom_ids = entry.custom_fields().map(|(id, _)| id).collect::<Vec<_>>();
    assert_eq!(
        database
            .with_entry_field_id(&key, &entry_id, custom_ids[0], str::to_owned)
            .unwrap(),
        "second"
    );
    assert_eq!(
        database
            .with_entry_field_id(&key, &entry_id, custom_ids[1], str::to_owned)
            .unwrap(),
        "first"
    );

    let mut bytes = Vec::new();
    save_database(&mut bytes, &database, &key).unwrap();
    let reopened = open_database(bytes.as_slice(), &key).unwrap();
    let reopened_ids = reopened
        .get_entry(&entry_id)
        .unwrap()
        .custom_fields()
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    assert_eq!(
        reopened
            .with_entry_field_id(&key, &entry_id, reopened_ids[0], str::to_owned)
            .unwrap(),
        "second"
    );
    assert_eq!(
        reopened
            .with_entry_field_id(&key, &entry_id, reopened_ids[1], str::to_owned)
            .unwrap(),
        "first"
    );
    assert_eq!(
        reopened
            .with_entry_field_id(&key, &entry_id, reopened_ids[2], str::to_owned)
            .unwrap(),
        "added"
    );

    let mut tracker = ChangeTracker::from_snapshot_with_credentials(&reopened, &key).unwrap();
    let mut changed = reopened.clone();
    let source = changed.get_entry(&entry_id).unwrap();
    let mut changed_fields = standard_fields(source);
    changed_fields.extend([
        EntryFieldUpdate {
            field_id: Some(source_field_id(source, "Renamed", 0)),
            name: "Renamed".into(),
            value: Some("changed-second".into()),
            is_protected: true,
        },
        EntryFieldUpdate {
            field_id: Some(source_field_id(source, "Duplicate", 0)),
            name: "Duplicate".into(),
            value: None,
            is_protected: true,
        },
        EntryFieldUpdate {
            field_id: Some(source_field_id(source, "Added", 0)),
            name: "Added".into(),
            value: None,
            is_protected: true,
        },
    ]);
    changed
        .update_entry_fields(&key, &entry_id, &changed_fields)
        .unwrap();
    let diff = tracker
        .diff_against_snapshot_with_credentials(&changed, &key)
        .unwrap();
    assert_eq!(diff.modified_entries, [entry_id]);
}

#[test]
fn invalid_and_noop_updates_are_atomic() {
    let (mut database, key, entry_id) = loaded_database();
    database.data_modified = false;
    let before = database.get_entry(&entry_id).unwrap().clone();
    let mut invalid = standard_fields(&before);
    invalid[0].name = "RenamedTitle".into();
    assert!(matches!(
        database.update_entry_fields(&key, &entry_id, &invalid),
        Err(DatabaseError::InvalidFormat(_))
    ));
    assert_eq!(database.get_entry(&entry_id).unwrap(), &before);
    assert!(!database.data_modified);

    let mut unchanged = standard_fields(&before);
    unchanged[0].value = Some("Old".into());
    unchanged.extend([
        EntryFieldUpdate {
            field_id: Some(source_field_id(&before, "Duplicate", 0)),
            name: "Duplicate".into(),
            value: None,
            is_protected: true,
        },
        EntryFieldUpdate {
            field_id: Some(source_field_id(&before, "Duplicate", 1)),
            name: "Duplicate".into(),
            value: None,
            is_protected: true,
        },
        EntryFieldUpdate {
            field_id: Some(source_field_id(&before, "Remove", 0)),
            name: "Remove".into(),
            value: Some("remove".into()),
            is_protected: false,
        },
    ]);
    assert!(!database
        .update_entry_fields(&key, &entry_id, &unchanged)
        .unwrap());
    assert_eq!(database.get_entry(&entry_id).unwrap(), &before);
    assert!(!database.data_modified);
}

#[test]
fn deleting_trailing_or_all_custom_fields_is_a_change() {
    for retained_occurrences in [&[0, 1][..], &[][..]] {
        let (mut database, key, entry_id) = loaded_database();
        database.data_modified = false;
        let source = database.get_entry(&entry_id).unwrap();
        let mut fields = standard_fields(source);
        fields[0].value = Some("Old".into());
        fields.extend(
            retained_occurrences
                .iter()
                .map(|occurrence| EntryFieldUpdate {
                    field_id: Some(source_field_id(source, "Duplicate", *occurrence)),
                    name: "Duplicate".into(),
                    value: None,
                    is_protected: true,
                }),
        );

        assert!(database
            .update_entry_fields(&key, &entry_id, &fields)
            .unwrap());
        let entry = database.get_entry(&entry_id).unwrap();
        assert_eq!(entry.custom_fields().count(), retained_occurrences.len());
        assert_eq!(entry.history.len(), 1);
        assert!(database.data_modified);
    }
}
