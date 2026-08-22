use keeless_kdbx::{
    initialize_database_key, open_database, open_database_with_key, save_database, ChangeTracker,
    CompositeCredentials, CompositeKey, Database, DatabaseError, DatabaseVersion, DateInstant,
    Entry, EntryFieldId, EntryFieldUpdate, EntryPropertiesUpdate, EntryUpdate, Group, IconImage,
    IconImageStandard, IconUpdate, NodeId, ProtectedString,
};

fn loaded_database() -> (Database, CompositeKey, NodeId) {
    let mut database = Database::new(DatabaseVersion::KDBX4);
    let credentials = CompositeCredentials::new().with_password(b"test").unwrap();
    let key = initialize_database_key(&mut database, &credentials).unwrap();
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
    let opened = open_database(bytes.as_slice(), &credentials).unwrap();
    (opened.database, opened.key, entry_id)
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

fn unchanged_fields(entry: &Entry) -> Vec<EntryFieldUpdate> {
    entry
        .fields()
        .map(|(id, field)| EntryFieldUpdate {
            field_id: Some(id),
            name: field.name().to_string(),
            value: (!field.value().is_protected()).then(|| field.value().as_str().to_string()),
            is_protected: field.value().is_protected(),
        })
        .collect()
}

fn entry_update(
    fields: Vec<EntryFieldUpdate>,
    properties: Option<EntryPropertiesUpdate>,
) -> EntryUpdate {
    EntryUpdate {
        fields,
        properties,
        attachments: vec![],
        removed_attachment_indices: vec![],
        last_modification_time: DateInstant::now(),
    }
}

#[test]
fn custom_field_ids_survive_serde_and_kdbx_round_trips() {
    let entry_id = NodeId::new_uuid();
    let mut entry = Entry::new(entry_id);
    entry.add_custom_field("Shared", ProtectedString::new_plain("first"));
    entry.add_custom_field("shared", ProtectedString::new_plain("case-sensitive"));
    entry.add_custom_field("Shared", ProtectedString::new_plain("second"));
    let expected_ids = entry.custom_fields().map(|(id, _)| id).collect::<Vec<_>>();

    let serialized = serde_json::to_string(&entry).unwrap();
    let deserialized: Entry = serde_json::from_str(&serialized).unwrap();
    assert_eq!(
        deserialized
            .custom_fields()
            .map(|(id, _)| id)
            .collect::<Vec<_>>(),
        expected_ids
    );
    assert_ne!(expected_ids[0], expected_ids[1]);
    assert_ne!(expected_ids[0], expected_ids[2]);

    let mut database = Database::new(DatabaseVersion::KDBX4);
    let credentials = CompositeCredentials::new().with_password(b"test").unwrap();
    let key = initialize_database_key(&mut database, &credentials).unwrap();
    let root_id = NodeId::new_uuid();
    database.groups.insert(root_id, Group::new(root_id));
    database.root_group_id = Some(root_id);
    database.add_entry(entry, &root_id);
    let mut bytes = Vec::new();
    save_database(&mut bytes, &database, &key).unwrap();
    let reopened = open_database(bytes.as_slice(), &credentials).unwrap();
    assert_eq!(
        reopened
            .get_entry(&entry_id)
            .unwrap()
            .custom_fields()
            .map(|(id, _)| id)
            .collect::<Vec<_>>(),
        expected_ids
    );
}

#[test]
fn update_reorders_renames_adds_and_deletes_without_confusing_duplicate_names() {
    let (mut database, key, entry_id) = loaded_database();
    let old_timestamp = database
        .get_entry(&entry_id)
        .unwrap()
        .last_modification_time;
    let source = database.get_entry(&entry_id).unwrap();
    let original_first_duplicate_id = source_field_id(source, "Duplicate", 0);
    let original_second_duplicate_id = source_field_id(source, "Duplicate", 1);
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
        .update_entry(&key, &entry_id, &entry_update(fields, None))
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
    assert_ne!(custom_ids[0], original_second_duplicate_id);
    assert_eq!(custom_ids[1], original_first_duplicate_id);
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
    let reopened = open_database_with_key(bytes.as_slice(), &key).unwrap();
    let reopened_ids = reopened
        .get_entry(&entry_id)
        .unwrap()
        .custom_fields()
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    assert_eq!(reopened_ids, custom_ids);
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
        .update_entry(&key, &entry_id, &entry_update(changed_fields, None))
        .unwrap();
    let diff = tracker
        .diff_against_snapshot_with_credentials(&changed, &key)
        .unwrap();
    assert_eq!(diff.modified_entries, [entry_id]);
}

#[test]
fn invalid_updates_are_atomic() {
    let (mut database, key, entry_id) = loaded_database();
    database.data_modified = false;
    let before = database.get_entry(&entry_id).unwrap().clone();
    let mut invalid = standard_fields(&before);
    invalid[0].name = "RenamedTitle".into();
    assert!(matches!(
        database.update_entry(&key, &entry_id, &entry_update(invalid, None)),
        Err(DatabaseError::InvalidFormat(_))
    ));
    assert_eq!(database.get_entry(&entry_id).unwrap(), &before);
    assert!(!database.data_modified);

    assert!(matches!(
        database.update_entry(
            &key,
            &entry_id,
            &entry_update(unchanged_fields(&before), None)
        ),
        Err(DatabaseError::InvalidFormat(_))
    ));
    assert_eq!(database.get_entry(&entry_id).unwrap(), &before);
    assert!(!database.data_modified);
}

#[test]
fn deleting_trailing_or_all_custom_fields_is_a_change() {
    for retain_duplicate in [true, false] {
        let (mut database, key, entry_id) = loaded_database();
        database.data_modified = false;
        let source = database.get_entry(&entry_id).unwrap();
        let mut fields = standard_fields(source);
        fields[0].value = Some("Old".into());
        if retain_duplicate {
            fields.push(EntryFieldUpdate {
                field_id: Some(source_field_id(source, "Duplicate", 0)),
                name: "Duplicate".into(),
                value: None,
                is_protected: true,
            });
        }

        assert!(database
            .update_entry(&key, &entry_id, &entry_update(fields, None))
            .unwrap());
        let entry = database.get_entry(&entry_id).unwrap();
        assert_eq!(
            entry.custom_fields().count(),
            if retain_duplicate { 1 } else { 0 }
        );
        assert_eq!(entry.history.len(), 1);
        assert!(database.data_modified);
    }
}

#[test]
fn template_metadata_can_be_updated_added_and_deleted() {
    let (mut database, key, entry_id) = loaded_database();
    let reserved_id = database.get_entry_mut(&entry_id).unwrap().add_custom_field(
        "_etm_template_uuid",
        ProtectedString::new_plain("00112233445566778899AABBCCDDEEFF"),
    );
    database.data_modified = false;
    let before = database.get_entry(&entry_id).unwrap().clone();
    let mut changed = unchanged_fields(&before);
    changed.retain(|field| field.name != "Duplicate");
    let reserved = changed
        .iter_mut()
        .find(|field| field.field_id == Some(reserved_id))
        .unwrap();
    reserved.name = "_etm_updated_uuid".into();
    reserved.value = Some("FFEEDDCCBBAA99887766554433221100".into());
    changed.push(EntryFieldUpdate {
        field_id: None,
        name: "_etm_client_value".into(),
        value: Some("allowed".into()),
        is_protected: true,
    });
    assert!(database
        .update_entry(&key, &entry_id, &entry_update(changed, None))
        .unwrap());
    let updated = database.get_entry(&entry_id).unwrap();
    assert!(updated.field(reserved_id).is_none());
    let renamed = updated
        .custom_fields()
        .find(|(_, field)| field.name() == "_etm_updated_uuid")
        .unwrap()
        .1;
    assert_eq!(renamed.name(), "_etm_updated_uuid");
    assert_eq!(renamed.value().as_str(), "FFEEDDCCBBAA99887766554433221100");
    assert!(updated
        .custom_fields()
        .any(|(_, field)| field.name() == "_etm_client_value" && field.value().is_protected()));

    let mut without_internal = unchanged_fields(updated);
    without_internal.retain(|field| !field.name.starts_with("_etm_"));
    assert!(database
        .update_entry(&key, &entry_id, &entry_update(without_internal, None))
        .unwrap());
    assert!(database
        .get_entry(&entry_id)
        .unwrap()
        .custom_fields()
        .all(|(_, field)| !field.name().starts_with("_etm_")));
}

#[test]
fn fields_and_properties_commit_with_one_history_snapshot() {
    let (mut database, key, entry_id) = loaded_database();
    let original = database.get_entry_mut(&entry_id).unwrap();
    original.override_url = "old-override".into();
    original.tags = vec!["old".into()];
    original.expires = false;
    original.expiry_time = DateInstant::EpochMillis(1234);
    original.icon = IconImage::Standard(IconImageStandard::new(5));
    original.custom_icon_uuid = Some(uuid::Uuid::from_u128(1));
    database.data_modified = false;

    let before = database.get_entry(&entry_id).unwrap().clone();
    let mut fields = unchanged_fields(&before);
    fields.retain(|field| field.name != "Duplicate");
    fields
        .iter_mut()
        .find(|field| field.name == "Title")
        .unwrap()
        .value = Some("Updated".into());
    let properties = EntryPropertiesUpdate {
        override_url: "new-override".into(),
        tags: vec!["one".into(), "two".into()],
        expires: true,
        expiry_time_ms: None,
        icon: Some(IconUpdate {
            standard_id: 12,
            custom_uuid: Some(uuid::Uuid::from_u128(2)),
        }),
    };

    assert!(database
        .update_entry(&key, &entry_id, &entry_update(fields, Some(properties)))
        .unwrap());
    let updated = database.get_entry(&entry_id).unwrap();
    assert_eq!(updated.title().as_str(), "Updated");
    assert_eq!(updated.override_url, "new-override");
    assert_eq!(updated.tags, ["one", "two"]);
    assert!(updated.expires);
    assert_eq!(updated.expiry_time, DateInstant::never());
    assert_eq!(
        updated.icon,
        IconImage::Standard(IconImageStandard::new(12))
    );
    assert_eq!(updated.custom_icon_uuid, Some(uuid::Uuid::from_u128(2)));
    assert_eq!(updated.history.len(), 1);
    assert_eq!(updated.history[0].title().as_str(), "Old");
    assert_eq!(updated.history[0].override_url, "old-override");
    assert_eq!(updated.history[0].tags, ["old"]);
    assert!(!updated.history[0].expires);
    assert_eq!(
        updated.history[0].icon,
        IconImage::Standard(IconImageStandard::new(5))
    );
    assert_eq!(
        updated.history[0].custom_icon_uuid,
        Some(uuid::Uuid::from_u128(1))
    );
    assert_eq!(
        updated.history[0].expiry_time,
        DateInstant::EpochMillis(1234)
    );
    assert!(database.data_modified);
}
