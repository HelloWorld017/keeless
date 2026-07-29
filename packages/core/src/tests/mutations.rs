use super::*;

#[tokio::test]
async fn update_entry_applies_one_atomic_history_change_and_preserves_duplicate_secrets() {
    let (mut core, ids) = query_core().await;
    let entry_id = schema_id(ids.root_entry);
    core.handle
        .as_mut()
        .unwrap()
        .database_mut()
        .get_entry_mut(&model_id(entry_id.clone()))
        .unwrap()
        .add_custom_field(
            "_etm_template_uuid",
            ProtectedString::new_plain(&Uuid::from_u128(50).to_string()),
        );
    let detail = operations::get_entry_detail::run(
        &mut core,
        GetEntryDetailArgs {
            entry_id: entry_id.clone(),
        },
    )
    .unwrap();
    let field_id = |name: &str, occurrence: usize| {
        detail
            .fields
            .iter()
            .filter_map(detail_field)
            .filter(|field| field.name == name)
            .nth(occurrence)
            .unwrap()
            .field_id
            .unwrap()
            .to_string()
    };
    let old = core
        .handle
        .as_ref()
        .unwrap()
        .database()
        .get_entry(&model_id(entry_id.clone()))
        .unwrap()
        .clone();
    let fields = vec![
        SchemaEntryFieldUpdate {
            field_id: Some(field_id("Title", 0)),
            name: "Title".into(),
            value: Some("Updated".into()),
            is_protected: false,
        },
        SchemaEntryFieldUpdate {
            field_id: Some(field_id("UserName", 0)),
            name: "UserName".into(),
            value: Some("alice".into()),
            is_protected: false,
        },
        SchemaEntryFieldUpdate {
            field_id: Some(field_id("Password", 0)),
            name: "Password".into(),
            value: None,
            is_protected: true,
        },
        SchemaEntryFieldUpdate {
            field_id: Some(field_id("URL", 0)),
            name: "URL".into(),
            value: Some("https://example.test".into()),
            is_protected: false,
        },
        SchemaEntryFieldUpdate {
            field_id: Some(field_id("Notes", 0)),
            name: "Notes".into(),
            value: None,
            is_protected: true,
        },
        SchemaEntryFieldUpdate {
            field_id: Some(field_id("Duplicate", 1)),
            name: "Renamed".into(),
            value: None,
            is_protected: true,
        },
        SchemaEntryFieldUpdate {
            field_id: Some(field_id("Duplicate", 0)),
            name: "Duplicate".into(),
            value: None,
            is_protected: true,
        },
        SchemaEntryFieldUpdate {
            field_id: None,
            name: "Added".into(),
            value: Some("new-secret".into()),
            is_protected: true,
        },
        SchemaEntryFieldUpdate {
            field_id: Some(field_id("_etm_template_uuid", 0)),
            name: "_etm_template_uuid".into(),
            value: Some(Uuid::from_u128(60).to_string()),
            is_protected: false,
        },
    ];
    operations::mutations::update_entry::run(
        &mut core,
        entry_id.clone(),
        fields,
        Some(EntryPropertiesUpdate {
            override_url: "https://override.test".into(),
            tags: vec!["updated".into()],
            expires: true,
            expiry_time_ms: Some(123_456),
            icon: Some(IconReference {
                standard_id: 12,
                custom_uuid: Some(Uuid::from_u128(100).hyphenated().to_string()),
            }),
        }),
        Vec::new(),
        None,
    )
    .await
    .unwrap();

    let entry = core
        .handle
        .as_ref()
        .unwrap()
        .database()
        .get_entry(&model_id(entry_id.clone()))
        .unwrap();
    assert_eq!(entry.history.len(), old.history.len() + 1);
    assert_eq!(entry.override_url, "https://override.test");
    assert_eq!(entry.tags, ["updated"]);
    assert!(entry.expires);
    assert_eq!(entry.expiry_time.as_millis(), Some(123_456));
    assert_eq!(
        entry.icon,
        keeless_kdbx::IconImage::Standard(IconImageStandard::new(12))
    );
    assert_eq!(entry.custom_icon_uuid, Some(Uuid::from_u128(100)));
    assert_eq!(
        entry.history.last().unwrap().icon,
        keeless_kdbx::IconImage::Standard(IconImageStandard::new(7))
    );
    assert_eq!(entry.history.last().unwrap().custom_icon_uuid, None);
    assert_eq!(
        entry
            .custom_fields()
            .find(|(_, field)| field.name() == "_etm_template_uuid")
            .unwrap()
            .1
            .value()
            .as_str(),
        Uuid::from_u128(60).to_string()
    );
    assert_eq!(
        entry.history.last().unwrap().last_modification_time,
        old.last_modification_time
    );
    assert_eq!(
        entry
            .custom_fields()
            .filter(|(_, field)| !template::is_internal_field(field.name()))
            .map(|(_, field)| field.name())
            .collect::<Vec<_>>(),
        ["Renamed", "Duplicate", "Added"]
    );
    let updated = operations::get_entry_detail::run(
        &mut core,
        GetEntryDetailArgs {
            entry_id: entry_id.clone(),
        },
    )
    .unwrap();
    for (name, expected) in [
        ("Renamed", "duplicate-second"),
        ("Duplicate", "duplicate-first"),
        ("Added", "new-secret"),
    ] {
        let field_id = updated
            .fields
            .iter()
            .filter_map(detail_field)
            .find(|field| field.name == name)
            .unwrap()
            .field_id
            .unwrap()
            .to_string();
        assert_eq!(
            operations::reveal_entry_fields::run(&mut core, entry_id.clone(), vec![field_id], None)
                .await
                .unwrap()
                .values,
            [expected]
        );
    }

    let before_failure = core
        .handle
        .as_ref()
        .unwrap()
        .database()
        .get_entry(&model_id(entry_id.clone()))
        .unwrap()
        .clone();
    assert!(matches!(
        operations::mutations::update_entry::run(
            &mut core,
            entry_id,
            vec![SchemaEntryFieldUpdate {
                field_id: Some(field_id("Title", 0)),
                name: "Wrong".into(),
                value: Some("bad".into()),
                is_protected: false
            }],
            Some(EntryPropertiesUpdate {
                override_url: "must-not-apply".into(),
                tags: vec![],
                expires: false,
                expiry_time_ms: None,
                icon: None,
            }),
            Vec::new(),
            None,
        )
        .await,
        Err(CoreError::InvalidEntryUpdate)
    ));
    assert_eq!(
        core.handle
            .as_ref()
            .unwrap()
            .database()
            .get_entry(&before_failure.id)
            .unwrap(),
        &before_failure
    );

    let current_fields = operations::get_entry_detail::run(
        &mut core,
        GetEntryDetailArgs {
            entry_id: schema_id(ids.root_entry),
        },
    )
    .unwrap()
    .fields
    .into_iter()
    .filter_map(|field| match field {
        EntryFieldInformation::Field {
            field_id,
            name,
            value,
            is_protected,
            ..
        } => Some(SchemaEntryFieldUpdate {
            field_id,
            name,
            value,
            is_protected,
        }),
        _ => None,
    })
    .collect::<Vec<_>>();
    operations::set_config::run(
        &mut core,
        KeelessConfigPatch {
            auto_lock_timeout_ms: None,
            paranoia_mode: Some(true),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        operations::mutations::update_entry::run(
            &mut core,
            schema_id(ids.root_entry),
            current_fields.clone(),
            None,
            Vec::new(),
            None,
        )
        .await,
        Err(CoreError::PasswordRequired)
    ));
    operations::mutations::update_entry::run(
        &mut core,
        schema_id(ids.root_entry),
        current_fields,
        None,
        Vec::new(),
        Some(b"correct"),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn update_entry_adds_transferred_attachments_and_download_verifies_name() {
    let (mut core, ids) = query_core().await;
    let transfers = Arc::new(MemoryTransferProvider::default());
    transfers.add_upload("upload-1", b"new attachment".to_vec());
    core.transfer_provider = Some(transfers.clone());
    core.transfer_owner = Some("test-client".into());
    let entry_id = schema_id(ids.root_entry);
    let fields = operations::get_entry_detail::run(
        &mut core,
        GetEntryDetailArgs {
            entry_id: entry_id.clone(),
        },
    )
    .unwrap()
    .fields
    .into_iter()
    .filter_map(|field| match field {
        EntryFieldInformation::Field {
            field_id,
            name,
            value,
            is_protected,
            ..
        } => Some(SchemaEntryFieldUpdate {
            field_id,
            name,
            value,
            is_protected,
        }),
        _ => None,
    })
    .collect();

    operations::mutations::update_entry::run(
        &mut core,
        entry_id.clone(),
        fields,
        None,
        vec![keeless_schema::EntryAttachmentUpdate {
            transfer_id: "upload-1".into(),
            name: "new.txt".into(),
        }],
        None,
    )
    .await
    .unwrap();

    let detail = operations::get_entry_detail::run(
        &mut core,
        GetEntryDetailArgs {
            entry_id: entry_id.clone(),
        },
    )
    .unwrap();
    let attachment = detail.attachments.last().unwrap();
    assert_eq!(attachment.name, "new.txt");
    assert_eq!(attachment.size, 14);
    assert!(matches!(
        operations::prepare_entry_attachment_download::execute(
            &mut core,
            keeless_schema::PrepareEntryAttachmentDownloadArgs {
                entry_id: entry_id.clone(),
                attachment_index: attachment.index,
                name: "wrong-name.txt".into(),
            },
        ),
        Err(CoreError::AttachmentNotFound)
    ));
    let OperationSuccess::PrepareEntryAttachmentDownload(download) =
        operations::prepare_entry_attachment_download::execute(
            &mut core,
            keeless_schema::PrepareEntryAttachmentDownloadArgs {
                entry_id,
                attachment_index: attachment.index,
                name: attachment.name.clone(),
            },
        )
        .unwrap()
    else {
        unreachable!();
    };
    assert_eq!(
        transfers.download(&download.transfer_id).unwrap(),
        b"new attachment"
    );
}

#[tokio::test]
async fn delete_entry_requires_trash_for_permanent_removal() {
    let (mut core, ids) = query_core().await;
    let entry_id = schema_id(ids.nested_entry);
    assert!(matches!(
        operations::mutations::delete_entry::run(
            &mut core,
            DeleteEntryArgs {
                entry_id: entry_id.clone(),
                permanent: true
            }
        )
        .await,
        Err(CoreError::InvalidEntryDelete)
    ));
    assert!(
        core.handle
            .as_ref()
            .unwrap()
            .database()
            .get_entry(&model_id(entry_id.clone()))
            .is_some()
    );

    operations::mutations::delete_entry::run(
        &mut core,
        DeleteEntryArgs {
            entry_id: entry_id.clone(),
            permanent: false,
        },
    )
    .await
    .unwrap();
    let database = core.handle.as_ref().unwrap().database();
    assert!(database.is_entry_in_recycle_bin(&model_id(entry_id.clone())));
    assert!(matches!(
        operations::mutations::delete_entry::run(
            &mut core,
            DeleteEntryArgs {
                entry_id: entry_id.clone(),
                permanent: false,
            }
        )
        .await,
        Err(CoreError::InvalidEntryDelete)
    ));
    operations::mutations::delete_entry::run(
        &mut core,
        DeleteEntryArgs {
            entry_id: entry_id.clone(),
            permanent: true,
        },
    )
    .await
    .unwrap();
    assert!(
        core.handle
            .as_ref()
            .unwrap()
            .database()
            .get_entry(&model_id(entry_id))
            .is_none()
    );
}

#[tokio::test]
async fn save_database_protocol_uses_sync_and_preserves_dirty_memory_on_failure() {
    let (mut core, ids) = query_core().await;
    core.handle
        .as_mut()
        .unwrap()
        .database_mut()
        .get_entry_mut(&NodeId::from_uuid(ids.root_entry))
        .unwrap()
        .set_title("unsaved");
    assert!(core.handle.as_ref().unwrap().is_dirty());
    assert!(
        operations::save_database::run(&mut core, None)
            .await
            .is_err()
    );
    assert!(core.handle.as_ref().unwrap().is_dirty());
    assert_eq!(
        core.handle
            .as_ref()
            .unwrap()
            .database()
            .get_entry(&NodeId::from_uuid(ids.root_entry))
            .unwrap()
            .title()
            .as_str(),
        "unsaved"
    );
}

#[tokio::test]
async fn add_and_rename_operations_validate_parents_and_apply_defaults() {
    let (mut core, ids) = query_core().await;
    let entry_count = core.handle.as_ref().unwrap().database().entry_count();
    let group_count = core.handle.as_ref().unwrap().database().group_count();
    let missing = schema_id(Uuid::from_u128(999));

    assert!(matches!(
        operations::mutations::add_entry::run(
            &mut core,
            AddEntryArgs {
                parent_group_id: missing.clone(),
            },
        )
        .await,
        Err(CoreError::GroupNotFound)
    ));
    assert!(matches!(
        operations::mutations::add_group::run(
            &mut core,
            AddGroupArgs {
                parent_group_id: missing,
            },
        )
        .await,
        Err(CoreError::GroupNotFound)
    ));
    assert_eq!(
        core.handle.as_ref().unwrap().database().entry_count(),
        entry_count
    );
    assert_eq!(
        core.handle.as_ref().unwrap().database().group_count(),
        group_count
    );

    let entry = operations::mutations::add_entry::run(
        &mut core,
        AddEntryArgs {
            parent_group_id: schema_id(ids.child_group),
        },
    )
    .await
    .unwrap();
    let detail =
        operations::get_entry_detail::run(&mut core, GetEntryDetailArgs { entry_id: entry.id })
            .unwrap();
    assert_eq!(detail_field(&detail.fields[0]).unwrap().value, Some(""));

    let group = operations::mutations::add_group::run(
        &mut core,
        AddGroupArgs {
            parent_group_id: schema_id(ids.child_group),
        },
    )
    .await
    .unwrap();
    operations::mutations::rename_group::run(
        &mut core,
        RenameGroupArgs {
            group_id: group.id.clone(),
            name: "  Renamed Group  ".into(),
        },
    )
    .await
    .unwrap();
    operations::mutations::update_group::run(
        &mut core,
        UpdateGroupArgs {
            group_id: group.id.clone(),
            name: "  Updated Group  ".into(),
            icon: IconReference {
                standard_id: 9,
                custom_uuid: Some(Uuid::from_u128(100).hyphenated().to_string()),
            },
        },
    )
    .await
    .unwrap();
    let hierarchy = operations::get_group_hierarchy::run(&mut core).unwrap();
    let created_group = hierarchy
        .groups
        .iter()
        .find(|candidate| candidate.id == group.id)
        .unwrap();
    assert_eq!(created_group.name, "Updated Group");
    assert_eq!(created_group.icon.standard_id, 9);
    assert_eq!(
        created_group.icon.custom_uuid.as_deref(),
        Some(Uuid::from_u128(100).hyphenated().to_string().as_str())
    );
    assert!(matches!(
        operations::mutations::rename_group::run(
            &mut core,
            RenameGroupArgs {
                group_id: schema_id(ids.child_group),
                name: "  ".into(),
            },
        )
        .await,
        Err(CoreError::InvalidGroupName)
    ));
    assert_eq!(
        keeless_schema::OperationError::from(&CoreError::InvalidGroupName).code,
        "invalid_group_name"
    );
}
