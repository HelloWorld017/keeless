use keeless_kdbx::kdbx::template::{
    instantiate_at, TemplateCopyMode, TemplateInstantiationOptions,
};
use keeless_kdbx::{
    CompositeKey, CustomData, Database, DatabaseVersion, DateInstant, Entry, EntryFieldId,
    EntryFieldUpdate, EntryUpdate, Group, NodeId, ProtectedString,
};
use uuid::Uuid;

fn database_at(timestamp: DateInstant) -> (Database, NodeId) {
    let mut database = Database::new(DatabaseVersion::KDBX4);
    let root_id = NodeId::from_uuid(Uuid::from_u128(1));
    assert!(database.add_group(
        Group::new_at(root_id, timestamp),
        &NodeId::from_uuid(Uuid::nil())
    ));
    database.data_modified = false;
    (database, root_id)
}

#[test]
fn constructors_group_edits_and_moves_use_supplied_timestamps() {
    let initial = DateInstant::EpochMillis(10);
    let changed = DateInstant::EpochMillis(20);
    let moved = DateInstant::EpochMillis(30);
    let (mut database, root_id) = database_at(initial);
    let first_id = NodeId::from_uuid(Uuid::from_u128(2));
    let second_id = NodeId::from_uuid(Uuid::from_u128(3));
    let entry_id = NodeId::from_uuid(Uuid::from_u128(4));

    database.add_group_validated(Group::new_at(first_id, initial), &root_id);
    database.add_group_validated(Group::new_at(second_id, initial), &root_id);
    database.add_entry_validated(Entry::new_at(entry_id, initial), &first_id);
    assert!(database.rename_group_at(&first_id, "renamed".into(), changed));
    assert!(database.reposition_entry_at(&entry_id, &second_id, moved));
    assert!(database.reposition_group_at(&second_id, &first_id, 0, moved));

    assert_eq!(
        database.get_group(&first_id).unwrap().creation_time,
        initial
    );
    assert_eq!(
        database
            .get_group(&first_id)
            .unwrap()
            .last_modification_time,
        changed
    );
    assert_eq!(
        database.get_group(&second_id).unwrap().location_changed,
        moved
    );
    assert_eq!(
        database.get_entry(&entry_id).unwrap().location_changed,
        moved
    );
    assert_eq!(
        database.find_parent_group_of_entry(&entry_id),
        Some(second_id)
    );
}

#[test]
fn prepared_entry_update_is_pure_and_commits_supplied_id_and_time() {
    let (mut database, root_id) = database_at(DateInstant::EpochMillis(1));
    let entry_id = NodeId::from_uuid(Uuid::from_u128(10));
    database.add_entry_validated(
        Entry::new_at(entry_id, DateInstant::EpochMillis(1)),
        &root_id,
    );
    database.data_modified = false;
    let before = database.get_entry(&entry_id).unwrap().clone();
    let mut fields = before
        .fields()
        .map(|(field_id, field)| EntryFieldUpdate {
            field_id: Some(field_id),
            name: field.name().to_string(),
            value: Some(field.value().as_str().to_string()),
            is_protected: field.value().is_protected(),
        })
        .collect::<Vec<_>>();
    fields.push(EntryFieldUpdate {
        field_id: None,
        name: "journal-field".into(),
        value: Some("value".into()),
        is_protected: false,
    });
    let custom_uuid = Uuid::from_u128(11);
    let modified = DateInstant::EpochMillis(99);
    let key = CompositeKey::new().with_password(b"test").unwrap();

    let prepared = database
        .prepare_entry_update(
            &key,
            &entry_id,
            &EntryUpdate {
                fields,
                properties: None,
                attachments: vec![],
                removed_attachment_indices: vec![],
                new_custom_field_ids: vec![custom_uuid],
                last_modification_time: modified,
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(database.get_entry(&entry_id).unwrap(), &before);
    assert!(!database.data_modified);

    database.commit_entry_update(prepared);
    let updated = database.get_entry(&entry_id).unwrap();
    assert_eq!(updated.last_modification_time, modified);
    assert_eq!(
        updated
            .field(EntryFieldId::Custom(custom_uuid))
            .unwrap()
            .name(),
        "journal-field"
    );
    assert_eq!(updated.history, vec![before]);
}

#[test]
fn recycle_and_permanent_delete_use_supplied_uuid_and_time() {
    let (mut database, root_id) = database_at(DateInstant::EpochMillis(1));
    let entry_id = NodeId::from_uuid(Uuid::from_u128(20));
    let group_id = NodeId::from_uuid(Uuid::from_u128(21));
    let recycle_uuid = Uuid::from_u128(22);
    database.add_group_validated(
        Group::new_at(group_id, DateInstant::EpochMillis(1)),
        &root_id,
    );
    database.add_entry_validated(
        Entry::new_at(entry_id, DateInstant::EpochMillis(1)),
        &group_id,
    );

    assert!(database.delete_group_at(&group_id, false, recycle_uuid, 50));
    assert_eq!(database.recycle_bin_uuid, Some(recycle_uuid));
    assert_eq!(
        database.get_group(&group_id).unwrap().location_changed,
        DateInstant::EpochMillis(50)
    );
    assert!(database.delete_group_at(&group_id, true, recycle_uuid, 60));
    assert!(database.get_group(&group_id).is_none());
    assert!(database.get_entry(&entry_id).is_none());
    assert!(database
        .deleted_objects
        .iter()
        .filter(|deleted| deleted.id == group_id || deleted.id == entry_id)
        .all(|deleted| deleted.deletion_time == 60));
}

#[test]
fn custom_data_and_template_instantiation_are_fully_deterministic() {
    let mut custom_data = CustomData::new();
    custom_data.set_at("key", "value", Some(123));
    assert_eq!(
        custom_data.iter().next().unwrap().1.last_modification_time,
        Some(123)
    );

    let timestamp = DateInstant::EpochMillis(777);
    let (mut database, root_id) = database_at(DateInstant::EpochMillis(1));
    let templates_uuid = Uuid::from_u128(30);
    let templates_id = NodeId::from_uuid(templates_uuid);
    let source_id = NodeId::from_uuid(Uuid::from_u128(31));
    let child_id = NodeId::from_uuid(Uuid::from_u128(32));
    let link_id = EntryFieldId::Custom(Uuid::from_u128(33));
    database.add_group_validated(Group::new_at(templates_id, timestamp), &root_id);
    database.entry_templates_uuid = Some(templates_uuid);
    let mut source = Entry::new_at(source_id, timestamp);
    source.set_password(ProtectedString::new_protected("secret"));
    source.add_custom_field("_etm_template", ProtectedString::new_plain("1"));
    database.add_entry_validated(source, &templates_id);

    assert_eq!(
        instantiate_at(
            &mut database,
            &source_id,
            &root_id,
            TemplateInstantiationOptions {
                new_entry_id: child_id,
                link_field_id: link_id,
                timestamp,
                copy_mode: TemplateCopyMode::RedactProtected,
                composite_key: None,
            },
        )
        .unwrap(),
        Some(child_id)
    );
    let child = database.get_entry(&child_id).unwrap();
    assert_eq!(child.creation_time, timestamp);
    assert_eq!(child.last_modification_time, timestamp);
    assert_eq!(child.password().as_str(), "");
    assert_eq!(child.field(link_id).unwrap().name(), "_etm_template_uuid");
}
