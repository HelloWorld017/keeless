use keeless_kdbx::{
    open_database, save_database, ChangeTracker, CompositeKey, Database, DatabaseError,
    DatabaseVersion, DateInstant, Entry, EntryField, EntryFieldUpdate, Group, NodeId,
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
    entry.title = "Old".into();
    entry.username = "user".into();
    entry.password = ProtectedString::new_protected("password");
    entry.url = "https://old.test".into();
    entry.notes = ProtectedString::new_protected("notes");
    entry.last_modification_time = DateInstant::EpochMillis(123);
    entry.custom_fields = vec![
        EntryField {
            name: "Duplicate".into(),
            value: ProtectedString::new_protected("first"),
        },
        EntryField {
            name: "Duplicate".into(),
            value: ProtectedString::new_protected("second"),
        },
        EntryField {
            name: "Remove".into(),
            value: ProtectedString::new_plain("remove"),
        },
    ];
    database.add_entry(entry, &root_id);
    let mut bytes = Vec::new();
    save_database(&mut bytes, &database, &key).unwrap();
    (
        open_database(bytes.as_slice(), &key).unwrap(),
        key,
        entry_id,
    )
}

fn standard_fields() -> Vec<EntryFieldUpdate> {
    vec![
        EntryFieldUpdate {
            field_index: Some(0),
            name: "Title".into(),
            value: Some("New".into()),
            is_protected: false,
        },
        EntryFieldUpdate {
            field_index: Some(1),
            name: "UserName".into(),
            value: Some("user".into()),
            is_protected: false,
        },
        EntryFieldUpdate {
            field_index: Some(2),
            name: "Password".into(),
            value: None,
            is_protected: true,
        },
        EntryFieldUpdate {
            field_index: Some(3),
            name: "URL".into(),
            value: Some("https://old.test".into()),
            is_protected: false,
        },
        EntryFieldUpdate {
            field_index: Some(4),
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
    let mut fields = standard_fields();
    fields.extend([
        EntryFieldUpdate {
            field_index: Some(6),
            name: "Renamed".into(),
            value: None,
            is_protected: true,
        },
        EntryFieldUpdate {
            field_index: Some(5),
            name: "Duplicate".into(),
            value: None,
            is_protected: true,
        },
        EntryFieldUpdate {
            field_index: None,
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
            .custom_fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["Renamed", "Duplicate", "Added"]
    );
    assert_eq!(entry.history.len(), 1);
    assert_eq!(entry.history[0].last_modification_time, old_timestamp);
    assert_ne!(entry.last_modification_time, old_timestamp);
    assert!(database.data_modified);
    assert_eq!(
        database
            .with_entry_custom_field(&key, &entry_id, 0, str::to_owned)
            .unwrap(),
        "second"
    );
    assert_eq!(
        database
            .with_entry_custom_field(&key, &entry_id, 1, str::to_owned)
            .unwrap(),
        "first"
    );

    let mut bytes = Vec::new();
    save_database(&mut bytes, &database, &key).unwrap();
    let reopened = open_database(bytes.as_slice(), &key).unwrap();
    assert_eq!(
        reopened
            .with_entry_custom_field(&key, &entry_id, 0, str::to_owned)
            .unwrap(),
        "second"
    );
    assert_eq!(
        reopened
            .with_entry_custom_field(&key, &entry_id, 1, str::to_owned)
            .unwrap(),
        "first"
    );
    assert_eq!(
        reopened
            .with_entry_custom_field(&key, &entry_id, 2, str::to_owned)
            .unwrap(),
        "added"
    );

    let mut tracker = ChangeTracker::from_snapshot_with_credentials(&reopened, &key).unwrap();
    let mut changed = reopened.clone();
    let mut changed_fields = vec![
        EntryFieldUpdate {
            field_index: Some(0),
            name: "Title".into(),
            value: Some("New".into()),
            is_protected: false,
        },
        EntryFieldUpdate {
            field_index: Some(1),
            name: "UserName".into(),
            value: Some("user".into()),
            is_protected: false,
        },
        EntryFieldUpdate {
            field_index: Some(2),
            name: "Password".into(),
            value: None,
            is_protected: true,
        },
        EntryFieldUpdate {
            field_index: Some(3),
            name: "URL".into(),
            value: Some("https://old.test".into()),
            is_protected: false,
        },
        EntryFieldUpdate {
            field_index: Some(4),
            name: "Notes".into(),
            value: None,
            is_protected: true,
        },
    ];
    changed_fields.extend([
        EntryFieldUpdate {
            field_index: Some(5),
            name: "Renamed".into(),
            value: Some("changed-second".into()),
            is_protected: true,
        },
        EntryFieldUpdate {
            field_index: Some(6),
            name: "Duplicate".into(),
            value: None,
            is_protected: true,
        },
        EntryFieldUpdate {
            field_index: Some(7),
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
    let mut invalid = standard_fields();
    invalid[0].name = "RenamedTitle".into();
    assert!(matches!(
        database.update_entry_fields(&key, &entry_id, &invalid),
        Err(DatabaseError::InvalidFormat(_))
    ));
    assert_eq!(database.get_entry(&entry_id).unwrap(), &before);
    assert!(!database.data_modified);

    let mut unchanged = standard_fields();
    unchanged[0].value = Some("Old".into());
    unchanged.extend([
        EntryFieldUpdate {
            field_index: Some(5),
            name: "Duplicate".into(),
            value: None,
            is_protected: true,
        },
        EntryFieldUpdate {
            field_index: Some(6),
            name: "Duplicate".into(),
            value: None,
            is_protected: true,
        },
        EntryFieldUpdate {
            field_index: Some(7),
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
    for retained_indices in [&[5, 6][..], &[][..]] {
        let (mut database, key, entry_id) = loaded_database();
        database.data_modified = false;
        let mut fields = standard_fields();
        fields[0].value = Some("Old".into());
        fields.extend(retained_indices.iter().map(|index| EntryFieldUpdate {
            field_index: Some(*index),
            name: "Duplicate".into(),
            value: None,
            is_protected: true,
        }));

        assert!(database
            .update_entry_fields(&key, &entry_id, &fields)
            .unwrap());
        let entry = database.get_entry(&entry_id).unwrap();
        assert_eq!(entry.custom_fields.len(), retained_indices.len());
        assert_eq!(entry.history.len(), 1);
        assert!(database.data_modified);
    }
}
