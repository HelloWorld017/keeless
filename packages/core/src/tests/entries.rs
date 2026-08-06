use super::*;

#[tokio::test]
async fn add_entry_from_template_uses_credentials_or_redacts_protected_content() {
    let (mut core, ids) = query_core().await;
    let key = core.credential.as_ref().unwrap().restore_key().unwrap();
    let templates_uuid = Uuid::from_u128(40);
    let templates_id = NodeId::from_uuid(templates_uuid);
    let template_id = NodeId::from_uuid(Uuid::from_u128(41));
    {
        let database = core.handle.as_mut().unwrap().database_mut();
        assert!(database.add_group(Group::new(templates_id), &NodeId::from_uuid(ids.root_group)));
        let mut template = Entry::new(template_id);
        template.set_title(ProtectedString::new_protected("Secret Template"));
        template.set_username(ProtectedString::new_plain("public-user"));
        template.set_password(ProtectedString::new_protected("secret-password"));
        template.add_custom_field(
            "Secret Field",
            ProtectedString::new_protected("secret-value"),
        );
        template.binaries = vec![
            EntryBinary {
                name: "public.txt".into(),
                data: b"public".to_vec(),
                is_protected: false,
            },
            EntryBinary {
                name: "secret.txt".into(),
                data: b"secret".to_vec(),
                is_protected: true,
            },
        ];
        template.history.push(Entry::new(template_id));
        template.usage_count = 9;
        template.add_custom_field("_etm_template", ProtectedString::new_plain("1"));
        template.add_custom_field("_etm_title_UserName", ProtectedString::new_plain("User"));
        assert!(database.add_entry(template, &templates_id));
        database.entry_templates_uuid = Some(templates_uuid);
        database.protect_entry_strings(&key).unwrap();
    }

    assert!(matches!(
        operations::mutations::add_entry_from_template::run(
            &mut core,
            AddEntryFromTemplateArgs {
                parent_group_id: schema_id(ids.child_group),
                template_entry_id: schema_id(ids.root_entry),
            },
        )
        .await,
        Err(CoreError::EntryNotFound)
    ));

    let copied_id = model_id(
        operations::mutations::add_entry_from_template::run(
            &mut core,
            AddEntryFromTemplateArgs {
                parent_group_id: schema_id(ids.child_group),
                template_entry_id: schema_id(Uuid::from_u128(41)),
            },
        )
        .await
        .unwrap()
        .id,
    );
    {
        let database = core.handle.as_ref().unwrap().database();
        assert_eq!(
            database
                .with_entry_field(
                    &key,
                    &copied_id,
                    &EntryFieldSelector::Password,
                    str::to_owned
                )
                .unwrap(),
            "secret-password"
        );
        let copied = database.get_entry(&copied_id).unwrap();
        assert_eq!(copied.binaries.len(), 2);
        assert!(copied.history.is_empty());
        assert_eq!(copied.usage_count, 0);
        assert!(template::is_template(database, &template_id));
        assert_eq!(
            copied
                .custom_fields()
                .filter(|(_, field)| template::is_template_field(field.name()))
                .map(|(_, field)| field.name())
                .collect::<Vec<_>>(),
            ["_etm_template_uuid"]
        );
    }

    let template_detail = operations::get_entry_detail::run(
        &mut core,
        GetEntryDetailArgs {
            entry_id: schema_id(Uuid::from_u128(41)),
        },
    )
    .unwrap();
    assert!(template_detail.is_template);
    assert!(
        template_detail
            .fields
            .iter()
            .filter_map(detail_field)
            .filter(|field| field.name.starts_with("_etm_"))
            .all(|field| field.is_internal)
    );
    let copied_detail = operations::get_entry_detail::run(
        &mut core,
        GetEntryDetailArgs {
            entry_id: crate::model::node_id(copied_id),
        },
    )
    .unwrap();
    assert!(!copied_detail.is_template);

    core.credential = None;
    let redacted_id = model_id(
        operations::mutations::add_entry_from_template::run(
            &mut core,
            AddEntryFromTemplateArgs {
                parent_group_id: schema_id(ids.child_group),
                template_entry_id: schema_id(Uuid::from_u128(41)),
            },
        )
        .await
        .unwrap()
        .id,
    );
    let database = core.handle.as_ref().unwrap().database();
    assert_eq!(
        database
            .with_entry_field(
                &key,
                &redacted_id,
                &EntryFieldSelector::Title,
                str::to_owned
            )
            .unwrap(),
        ""
    );
    let redacted = database.get_entry(&redacted_id).unwrap();
    assert_eq!(redacted.username().as_str(), "public-user");
    assert_eq!(redacted.password().as_str(), "");
    assert_eq!(
        redacted.custom_fields().next().unwrap().1.value().as_str(),
        ""
    );
    assert_eq!(redacted.binaries.len(), 1);
    assert_eq!(redacted.binaries[0].name, "public.txt");
    assert_eq!(
        database
            .with_entry_field(
                &key,
                &template_id,
                &EntryFieldSelector::Password,
                str::to_owned
            )
            .unwrap(),
        "secret-password"
    );
}

#[tokio::test]
async fn trash_and_tag_queries_filter_recursively_and_preserve_order() {
    let (mut core, ids) = query_core().await;
    let trash_entry_id = NodeId::from_uuid(Uuid::from_u128(20));
    let trash_group_id = NodeId::from_uuid(Uuid::from_u128(21));
    let nested_trash_entry_id = NodeId::from_uuid(Uuid::from_u128(22));
    {
        let database = core.handle.as_mut().unwrap().database_mut();
        database
            .get_entry_mut(&NodeId::from_uuid(ids.child_entry))
            .unwrap()
            .tags
            .push(" Work ".into());
        let recycle_bin_id = database.create_recycle_bin();
        let mut trash_entry = Entry::new(trash_entry_id);
        trash_entry.set_title("Trash");
        trash_entry.set_username(ProtectedString::new_plain("trashed-user"));
        trash_entry.tags = vec![" work ".into(), "TrashOnly".into(), "".into()];
        assert!(database.add_entry(trash_entry, &recycle_bin_id));
        assert!(database.add_group(Group::new(trash_group_id), &recycle_bin_id));
        let mut nested_trash_entry = Entry::new(nested_trash_entry_id);
        nested_trash_entry.set_title("Nested Trash");
        nested_trash_entry.tags = vec!["nested".into()];
        assert!(database.add_entry(nested_trash_entry, &trash_group_id));
    }

    for args in [
        GetEntriesArgs::default(),
        GetEntriesArgs {
            exclude_trash: true,
        },
    ] {
        assert_eq!(
            operations::get_entries::run(&mut core, args)
                .unwrap()
                .entries
                .into_iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            vec![
                schema_id(ids.root_entry),
                schema_id(ids.child_entry),
                schema_id(ids.nested_entry),
            ]
        );
    }
    assert_eq!(
        operations::get_entries::run(
            &mut core,
            GetEntriesArgs {
                exclude_trash: false,
            },
        )
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| entry.id)
        .collect::<Vec<_>>(),
        vec![
            schema_id(ids.root_entry),
            schema_id(ids.child_entry),
            schema_id(ids.nested_entry),
            schema_id(Uuid::from_u128(20)),
            schema_id(Uuid::from_u128(22)),
        ]
    );
    let trash_entries = operations::get_trash_entries::run(&mut core)
        .unwrap()
        .entries;
    assert_eq!(
        trash_entries
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>(),
        vec![
            schema_id(Uuid::from_u128(20)),
            schema_id(Uuid::from_u128(22)),
        ]
    );
    assert_eq!(trash_entries[0].username.as_deref(), Some("trashed-user"));
    assert_eq!(trash_entries[0].tags, vec![" work ", "TrashOnly", ""]);

    let work_entries =
        operations::get_tag_entries::run(&mut core, GetTagEntriesArgs { tag: "work".into() })
            .unwrap()
            .entries;
    assert_eq!(work_entries.len(), 1);
    assert_eq!(work_entries[0].username.as_deref(), Some("alice"));
    assert_eq!(work_entries[0].tags, vec!["shared", "work", "shared"]);

    let tagged = |core: &mut KeelessCore, tag: &str| {
        operations::get_tag_entries::run(core, GetTagEntriesArgs { tag: tag.into() })
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(tagged(&mut core, "work"), vec![schema_id(ids.root_entry)]);
    assert_eq!(tagged(&mut core, "Work"), vec![schema_id(ids.child_entry)]);
    assert!(tagged(&mut core, " work ").is_empty());
    assert!(tagged(&mut core, "TrashOnly").is_empty());

    let tags = operations::get_tags::run(&mut core).unwrap().tags;
    assert!(
        tags.iter()
            .any(|tag| tag.name == "Work" && tag.entry_count == 1)
    );
    assert!(!tags.iter().any(|tag| tag.name == "TrashOnly"));

    core.handle
        .as_mut()
        .unwrap()
        .database_mut()
        .recycle_bin_uuid = Some(Uuid::from_u128(999));
    assert!(
        operations::get_trash_entries::run(&mut core)
            .unwrap()
            .entries
            .is_empty()
    );
}

#[tokio::test]
async fn move_entry_operation_supports_trash_boundaries_and_rejects_invalid_moves() {
    let (mut core, ids) = query_core().await;
    let recycle_bin_id = core
        .handle
        .as_mut()
        .unwrap()
        .database_mut()
        .create_recycle_bin();

    operations::mutations::move_entry::run(
        &mut core,
        MoveEntryArgs {
            entry_id: schema_id(ids.root_entry),
            parent_group_id: schema_id(match recycle_bin_id {
                NodeId::Uuid(id) => id,
                NodeId::Int(_) => unreachable!(),
            }),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        operations::get_trash_entries::run(&mut core)
            .unwrap()
            .entries[0]
            .id,
        schema_id(ids.root_entry)
    );

    operations::mutations::move_entry::run(
        &mut core,
        MoveEntryArgs {
            entry_id: schema_id(ids.root_entry),
            parent_group_id: schema_id(ids.nested_group),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        operations::get_group_entries::run(
            &mut core,
            GetGroupEntriesArgs {
                group_id: schema_id(ids.nested_group),
            },
        )
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| entry.id)
        .collect::<Vec<_>>(),
        vec![schema_id(ids.nested_entry), schema_id(ids.root_entry)]
    );

    let orphan_id = NodeId::from_uuid(Uuid::from_u128(999));
    core.handle
        .as_mut()
        .unwrap()
        .database_mut()
        .entries
        .insert(orphan_id, Entry::new(orphan_id));
    assert!(matches!(
        operations::mutations::move_entry::run(
            &mut core,
            MoveEntryArgs {
                entry_id: schema_id(Uuid::from_u128(999)),
                parent_group_id: schema_id(ids.root_group),
            },
        )
        .await,
        Err(CoreError::InvalidEntryMove)
    ));
    assert!(matches!(
        operations::mutations::move_entry::run(
            &mut core,
            MoveEntryArgs {
                entry_id: schema_id(Uuid::from_u128(998)),
                parent_group_id: schema_id(ids.root_group),
            },
        )
        .await,
        Err(CoreError::EntryNotFound)
    ));
}

#[tokio::test]
async fn move_group_operation_reparents_reorders_and_rejects_cycles() {
    let (mut core, ids) = query_core().await;

    operations::mutations::move_group::run(
        &mut core,
        MoveGroupArgs {
            group_id: schema_id(ids.nested_group),
            parent_group_id: schema_id(ids.root_group),
            destination_index: 0,
        },
    )
    .await
    .unwrap();
    let hierarchy = operations::get_group_hierarchy::run(&mut core).unwrap();
    let root = hierarchy
        .groups
        .iter()
        .find(|group| group.id == schema_id(ids.root_group))
        .unwrap();
    assert_eq!(
        root.child_group_ids,
        vec![schema_id(ids.nested_group), schema_id(ids.child_group)]
    );

    assert!(matches!(
        operations::mutations::move_group::run(
            &mut core,
            MoveGroupArgs {
                group_id: schema_id(ids.child_group),
                parent_group_id: schema_id(ids.child_group),
                destination_index: 0,
            },
        )
        .await,
        Err(CoreError::InvalidGroupMove)
    ));
    assert!(matches!(
        operations::mutations::move_group::run(
            &mut core,
            MoveGroupArgs {
                group_id: schema_id(ids.root_group),
                parent_group_id: schema_id(ids.child_group),
                destination_index: 0,
            },
        )
        .await,
        Err(CoreError::InvalidGroupMove)
    ));
}

#[tokio::test]
async fn delete_group_operation_moves_the_subtree_to_trash_and_protects_special_groups() {
    let (mut core, ids) = query_core().await;
    let missing = schema_id(Uuid::from_u128(999));

    assert!(matches!(
        operations::mutations::delete_group::run(&mut core, DeleteGroupArgs { group_id: missing },)
            .await,
        Err(CoreError::GroupNotFound)
    ));
    assert!(matches!(
        operations::mutations::delete_group::run(
            &mut core,
            DeleteGroupArgs {
                group_id: schema_id(ids.root_group),
            },
        )
        .await,
        Err(CoreError::InvalidGroupDelete)
    ));

    operations::mutations::delete_group::run(
        &mut core,
        DeleteGroupArgs {
            group_id: schema_id(ids.child_group),
        },
    )
    .await
    .unwrap();

    let database = core.handle.as_ref().unwrap().database();
    let recycle_bin_id = NodeId::from_uuid(database.recycle_bin_uuid.unwrap());
    assert_eq!(
        database.get_group(&recycle_bin_id).unwrap().child_group_ids,
        vec![NodeId::from_uuid(ids.child_group)]
    );
    assert_eq!(
        operations::get_trash_entries::run(&mut core)
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec![schema_id(ids.child_entry), schema_id(ids.nested_entry)]
    );
    assert!(matches!(
        operations::mutations::delete_group::run(
            &mut core,
            DeleteGroupArgs {
                group_id: schema_id(match recycle_bin_id {
                    NodeId::Uuid(id) => id,
                    NodeId::Int(_) => unreachable!(),
                }),
            },
        )
        .await,
        Err(CoreError::InvalidGroupDelete)
    ));
}

#[tokio::test]
async fn entry_detail_redacts_protected_values_and_binary_contents() {
    let (mut core, ids) = query_core().await;
    let result = operations::get_entry_detail::run(
        &mut core,
        GetEntryDetailArgs {
            entry_id: schema_id(ids.root_entry),
        },
    )
    .unwrap();

    let field = |name: &str| {
        result
            .fields
            .iter()
            .filter_map(detail_field)
            .find(|field| field.name == name)
            .unwrap()
    };
    assert_eq!(field("Title").value, Some("Root Entry"));
    assert_eq!(field("Title").field_id, Some("standard:Title"));
    assert_eq!(field("Title").kind, &keeless_schema::EntryFieldKind::Title);
    assert_eq!(field("UserName").field_id, Some("standard:UserName"));
    assert_eq!(field("Password").field_id, Some("standard:Password"));
    assert_eq!(field("URL").field_id, Some("standard:URL"));
    assert_eq!(field("Notes").field_id, Some("standard:Notes"));
    assert_eq!(
        field("Public").kind,
        &keeless_schema::EntryFieldKind::Custom
    );
    assert!(Uuid::parse_str(field("Public").field_id.unwrap()).is_ok());
    assert_eq!(field("UserName").value, Some("alice"));
    assert_eq!(field("URL").value, Some("https://example.test"));
    assert_eq!(field("Public").value, Some("public-value"));
    for name in ["Password", "Notes", "Secret"] {
        assert_eq!(field(name).value, None);
        assert!(field(name).is_protected);
    }
    assert_eq!(result.background_color, "#000000");
    assert_eq!(result.foreground_color, "#ffffff");
    assert_eq!(result.override_url, "cmd://open");
    assert_eq!(result.usage_count, 3);
    assert_eq!(result.attachments.len(), 1);
    assert_eq!(result.attachments[0].name, "secret.bin");
    assert_eq!(result.attachments[0].size, 17);
    assert!(result.attachments[0].is_protected);

    let serialized = serde_json::to_string(&result).unwrap();
    for secret in [
        "password-secret",
        "notes-secret",
        "custom-secret",
        "duplicate-first",
        "duplicate-second",
        "attachment-secret",
    ] {
        assert!(!serialized.contains(secret));
    }

    let protected_title = operations::get_entry_detail::run(
        &mut core,
        GetEntryDetailArgs {
            entry_id: schema_id(ids.child_entry),
        },
    )
    .unwrap();
    let title = protected_title
        .fields
        .iter()
        .filter_map(detail_field)
        .find(|field| field.name == "Title")
        .unwrap();
    assert_eq!(title.value, None);
    assert!(title.is_protected);
    assert!(
        !serde_json::to_string(&protected_title)
            .unwrap()
            .contains("protected-title-secret")
    );
}

#[tokio::test]
async fn entry_detail_resolves_layout_and_ignores_invalid_links() {
    let (mut core, ids) = query_core().await;
    let template_uuid = Uuid::from_u128(50);
    let template_id = NodeId::from_uuid(template_uuid);
    {
        let database = core.handle.as_mut().unwrap().database_mut();
        let templates_uuid = Uuid::from_u128(51);
        let templates_id = NodeId::from_uuid(templates_uuid);
        assert!(database.add_group(Group::new(templates_id), &NodeId::from_uuid(ids.root_group)));
        database.entry_templates_uuid = Some(templates_uuid);
        let mut template = Entry::new(template_id);
        template.add_custom_field("_etm_template", ProtectedString::new_plain("1"));
        for (storage, label, field_type, position, options) in [
            ("Title", "Name", "Inline", 1, "2"),
            ("Password", "Secret", "Protected Inline", 2, "1"),
            ("URL", "Site", "Inline URL", 3, ""),
            ("Public", "Kind", "Listbox", 4, "one, two"),
            ("section", "Details", "Divider", 5, ""),
            ("@confirm", "Confirm", "Protected Inline", 6, "1"),
            ("@override", "Override", "Inline URL", 7, ""),
            ("@exp_date", "Expires", "Date", 8, ""),
            ("@tags", "Tags", "Inline", 9, "1"),
            ("@future", "Future", "Inline", 10, "1"),
        ] {
            template.add_custom_field(
                format!("_etm_title_{storage}"),
                ProtectedString::new_plain(label),
            );
            template.add_custom_field(
                format!("_etm_type_{storage}"),
                ProtectedString::new_plain(field_type),
            );
            template.add_custom_field(
                format!("_etm_position_{storage}"),
                ProtectedString::new_plain(&position.to_string()),
            );
            template.add_custom_field(
                format!("_etm_options_{storage}"),
                ProtectedString::new_plain(options),
            );
        }
        assert!(database.add_entry(template, &templates_id));
        database
            .get_entry_mut(&NodeId::from_uuid(ids.root_entry))
            .unwrap()
            .add_custom_field(
                "_etm_template_uuid",
                ProtectedString::new_plain(&template_uuid.to_string()),
            );
        database
            .get_entry_mut(&NodeId::from_uuid(ids.child_entry))
            .unwrap()
            .add_custom_field(
                "_etm_template_uuid",
                ProtectedString::new_plain("malformed"),
            );
        database
            .get_entry_mut(&NodeId::from_uuid(ids.nested_entry))
            .unwrap()
            .add_custom_field(
                "_etm_template_uuid",
                ProtectedString::new_plain(&Uuid::from_u128(999).to_string()),
            );
    }

    let detail = operations::get_entry_detail::run(
        &mut core,
        GetEntryDetailArgs {
            entry_id: schema_id(ids.root_entry),
        },
    )
    .unwrap();
    let actual = detail
        .fields
        .iter()
        .filter_map(detail_field)
        .collect::<Vec<_>>();
    assert!(
        actual
            .iter()
            .any(|field| field.name == "_etm_template_uuid")
    );
    assert!(
        actual
            .iter()
            .find(|field| field.name == "_etm_template_uuid")
            .unwrap()
            .is_internal
    );
    assert!(
        actual
            .iter()
            .filter(|field| !field.name.starts_with("_etm_"))
            .all(|field| !field.is_internal)
    );
    assert_eq!(
        actual
            .iter()
            .filter(|field| field.order < 5)
            .map(|field| field.name)
            .collect::<Vec<_>>(),
        ["Title", "UserName", "Password", "URL", "Notes"]
    );
    let title = actual.iter().find(|field| field.name == "Title").unwrap();
    assert_eq!(title.label, "Name");
    assert_eq!(
        title.control,
        Some(&FieldControl::Text {
            protected: false,
            lines: 2,
        })
    );
    let public = actual.iter().find(|field| field.name == "Public").unwrap();
    assert_eq!(public.label, "Kind");
    assert_eq!(
        public.control,
        Some(&FieldControl::Select {
            options: vec!["one".into(), "two".into()],
        })
    );
    let ordered_types = detail
        .fields
        .iter()
        .filter(|field| !matches!(field, EntryFieldInformation::Field { .. }))
        .map(|field| match field {
            EntryFieldInformation::PasswordConfirmation { .. } => "confirmation",
            EntryFieldInformation::OverrideUrl { .. } => "override",
            EntryFieldInformation::Expiry { .. } => "expiry",
            EntryFieldInformation::Tags { .. } => "tags",
            EntryFieldInformation::Divider { .. } => "divider",
            EntryFieldInformation::TimeOtp { .. } => "timeOtp",
            EntryFieldInformation::Field { .. } => unreachable!(),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ordered_types,
        ["divider", "confirmation", "override", "expiry", "tags"]
    );
    assert!(detail.fields.iter().all(|field| match field {
        EntryFieldInformation::Field { order, .. }
        | EntryFieldInformation::TimeOtp { order, .. }
        | EntryFieldInformation::PasswordConfirmation { order, .. }
        | EntryFieldInformation::OverrideUrl { order, .. }
        | EntryFieldInformation::Expiry { order, .. }
        | EntryFieldInformation::Tags { order, .. }
        | EntryFieldInformation::Divider { order, .. } => (*order as usize) < detail.fields.len(),
    }));
    for entry_id in [ids.child_entry, ids.nested_entry] {
        let invalid = operations::get_entry_detail::run(
            &mut core,
            GetEntryDetailArgs {
                entry_id: schema_id(entry_id),
            },
        )
        .unwrap();
        assert!(
            invalid.fields.iter().all(|field| {
                matches!(field, EntryFieldInformation::Field { control: None, .. })
            })
        );
        assert!(
            invalid
                .fields
                .iter()
                .filter_map(detail_field)
                .any(|field| { field.name == "_etm_template_uuid" })
        );
    }
}
