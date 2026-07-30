use super::*;

#[tokio::test]
async fn database_query_operations_preserve_hierarchy_order_and_group_scope() {
    let (mut core, ids) = query_core().await;

    let entries = operations::get_entries::run(&mut core, GetEntriesArgs::default())
        .unwrap()
        .entries;
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>(),
        vec![
            schema_id(ids.root_entry),
            schema_id(ids.child_entry),
            schema_id(ids.nested_entry),
        ]
    );
    let protected = entries
        .iter()
        .find(|entry| entry.id == schema_id(ids.child_entry))
        .unwrap();
    assert_eq!(protected.name, None);
    assert!(protected.name_is_protected);
    assert_eq!(protected.username, None);
    assert!(protected.username_is_protected);
    assert_eq!(protected.url, None);
    assert!(protected.url_is_protected);
    assert_eq!(
        protected.icon.custom_uuid,
        Some(Uuid::from_u128(100).to_string())
    );
    let root_entry = entries
        .iter()
        .find(|entry| entry.id == schema_id(ids.root_entry))
        .unwrap();
    assert_eq!(root_entry.url.as_deref(), Some("https://example.test"));
    assert!(!root_entry.url_is_protected);
    assert_eq!(root_entry.username.as_deref(), Some("alice"));
    assert!(!root_entry.username_is_protected);
    assert_eq!(root_entry.tags, vec!["shared", "work", "shared"]);

    let hierarchy = operations::get_group_hierarchy::run(&mut core).unwrap();
    assert_eq!(hierarchy.database_name, "Test Database");
    assert_eq!(hierarchy.recycle_bin_id, None);
    assert_eq!(hierarchy.root_group_id, schema_id(ids.root_group));
    assert_eq!(
        hierarchy
            .groups
            .iter()
            .map(|group| group.id.clone())
            .collect::<Vec<_>>(),
        vec![
            schema_id(ids.root_group),
            schema_id(ids.child_group),
            schema_id(ids.nested_group),
        ]
    );
    let child_group = hierarchy
        .groups
        .iter()
        .find(|group| group.id == schema_id(ids.child_group))
        .unwrap();
    assert_eq!(
        child_group.child_group_ids,
        vec![schema_id(ids.nested_group)]
    );
    assert_eq!(
        child_group.icon.custom_uuid,
        Some(Uuid::from_u128(100).to_string())
    );

    let group_entries = operations::get_group_entries::run(
        &mut core,
        GetGroupEntriesArgs {
            group_id: schema_id(ids.child_group),
        },
    )
    .unwrap();
    assert_eq!(group_entries.entries.len(), 1);
    assert_eq!(group_entries.entries[0].id, schema_id(ids.child_entry));
    assert_eq!(group_entries.entries[0].username, None);
    assert!(group_entries.entries[0].username_is_protected);
    assert_eq!(group_entries.entries[0].tags, vec!["shared"]);

    let tags = operations::get_tags::run(&mut core).unwrap();
    assert_eq!(
        tags.tags
            .into_iter()
            .map(|tag| (tag.name, tag.entry_count))
            .collect::<Vec<_>>(),
        vec![
            ("nested".into(), 1),
            ("shared".into(), 2),
            ("work".into(), 1),
        ]
    );

    let icons = operations::get_custom_icons::run(&mut core).unwrap();
    assert_eq!(icons.icons.len(), 1);
    assert_eq!(icons.icons[0].uuid, Uuid::from_u128(100).to_string());
    assert_eq!(icons.icons[0].data_base64, "AQID");
    assert_eq!(icons.icons[0].name, "Custom");
    assert_eq!(icons.icons[0].last_modification_time_ms, 1_700_000_000_000);
}

#[tokio::test]
async fn tag_styles_union_hidden_usage_and_validate_mutations() {
    let (mut core, ids) = query_core().await;
    let trash_group_id = NodeId::from_uuid(Uuid::from_u128(70));
    let template_group_id = NodeId::from_uuid(Uuid::from_u128(71));
    {
        let database = core.handle.as_mut().unwrap().database_mut();
        assert!(database.add_group(
            Group::new(trash_group_id),
            &NodeId::from_uuid(ids.root_group)
        ));
        assert!(database.add_group(
            Group::new(template_group_id),
            &NodeId::from_uuid(ids.root_group)
        ));
        let mut trash = Entry::new(NodeId::from_uuid(Uuid::from_u128(72)));
        trash.tags = vec![" TrashOnly ".into()];
        assert!(database.add_entry(trash, &trash_group_id));
        let mut template = Entry::new(NodeId::from_uuid(Uuid::from_u128(73)));
        template.tags = vec!["TemplateOnly".into()];
        template.is_template = true;
        assert!(database.add_entry(template, &template_group_id));
        database.recycle_bin_uuid = Some(Uuid::from_u128(70));
        database.entry_templates_uuid = Some(Uuid::from_u128(71));
    }

    let style = |standard_id, color: &str| TagStyle {
        icon: IconReference {
            standard_id,
            custom_uuid: None,
        },
        color: color.into(),
    };
    for (name, style) in [
        ("orphan", style(1, "#AABBCC")),
        ("shared", style(2, "#112233")),
        ("TrashOnly", style(3, "#445566")),
        ("TemplateOnly", style(4, "#778899")),
    ] {
        operations::mutations::update_tag_style::run(
            &mut core,
            UpdateTagStyleArgs {
                name: name.into(),
                style,
            },
        )
        .await
        .unwrap();
    }

    let tags = operations::get_tags::run(&mut core).unwrap().tags;
    assert!(tags.windows(2).all(|pair| pair[0].name < pair[1].name));
    let tag = |name: &str| tags.iter().find(|tag| tag.name == name).unwrap();
    assert_eq!(tag("orphan").entry_count, 0);
    assert_eq!(tag("orphan").style.as_ref().unwrap().color, "#aabbcc");
    assert!(tag("orphan").can_delete);
    assert!(!tag("shared").can_delete);
    assert_eq!(tag("TrashOnly").entry_count, 0);
    assert!(!tag("TrashOnly").can_delete);
    assert_eq!(tag("TemplateOnly").entry_count, 0);
    assert!(!tag("TemplateOnly").can_delete);

    operations::mutations::delete_tag::run(
        &mut core,
        DeleteTagArgs {
            name: " orphan ".into(),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        operations::mutations::delete_tag::run(
            &mut core,
            DeleteTagArgs {
                name: "shared".into()
            }
        )
        .await,
        Err(CoreError::TagInUse)
    ));
    assert!(matches!(
        operations::mutations::update_tag_style::run(
            &mut core,
            UpdateTagStyleArgs {
                name: "bad".into(),
                style: style(1, "red"),
            }
        )
        .await,
        Err(CoreError::InvalidTagStyle)
    ));
    assert!(matches!(
        operations::mutations::update_tag_style::run(
            &mut core,
            UpdateTagStyleArgs {
                name: "bad".into(),
                style: style(69, "#000000"),
            }
        )
        .await,
        Err(CoreError::InvalidIconReference)
    ));

    core.handle
        .as_mut()
        .unwrap()
        .database_mut()
        .custom_data
        .set("KLSS_TAG_STYLES", "not json");
    assert!(operations::get_tags::run(&mut core).is_ok());
    assert!(matches!(
        operations::mutations::update_tag_style::run(
            &mut core,
            UpdateTagStyleArgs {
                name: "new".into(),
                style: style(1, "#000000"),
            }
        )
        .await,
        Err(CoreError::MalformedTagStyles)
    ));
}

#[tokio::test]
async fn template_queries_are_direct_and_excluded_from_regular_results() {
    let (mut core, ids) = query_core().await;
    let templates_uuid = Uuid::from_u128(30);
    let templates_id = NodeId::from_uuid(templates_uuid);
    let nested_group_id = NodeId::from_uuid(Uuid::from_u128(31));
    let direct_template_id = NodeId::from_uuid(Uuid::from_u128(32));
    let nested_template_id = NodeId::from_uuid(Uuid::from_u128(33));
    {
        let database = core.handle.as_mut().unwrap().database_mut();
        let mut templates = Group::new(templates_id);
        templates.title = "Templates".into();
        templates.enable_searching = false;
        assert!(database.add_group(templates, &NodeId::from_uuid(ids.root_group)));
        assert!(database.add_group(Group::new(nested_group_id), &templates_id));

        let mut direct = Entry::new(direct_template_id);
        direct.set_title("Direct Template");
        direct.tags = vec!["template-only".into()];
        direct.add_custom_field("_etm_template", ProtectedString::new_plain("1"));
        assert!(database.add_entry(direct, &templates_id));

        let mut nested = Entry::new(nested_template_id);
        nested.set_title("Nested Template");
        nested.tags = vec!["template-only".into()];
        nested.add_custom_field("_etm_template", ProtectedString::new_plain("1"));
        assert!(database.add_entry(nested, &nested_group_id));
        for (offset, marker) in [None, Some("true"), Some("0"), Some("01"), Some(" 1")]
            .into_iter()
            .enumerate()
        {
            let id = NodeId::from_uuid(Uuid::from_u128(34 + offset as u128));
            let mut entry = Entry::new(id);
            entry.set_title("Not a template");
            if let Some(marker) = marker {
                entry.add_custom_field("_etm_template", ProtectedString::new_plain(marker));
            }
            assert!(database.add_entry(entry, &templates_id));
        }
        database.entry_templates_uuid = Some(templates_uuid);
    }

    let templates = operations::get_entry_templates::run(&mut core)
        .unwrap()
        .entries;
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0].id, schema_id(Uuid::from_u128(32)));
    assert!(
        operations::get_entries::run(&mut core, GetEntriesArgs::default())
            .unwrap()
            .entries
            .iter()
            .all(|entry| entry.id != schema_id(Uuid::from_u128(32))
                && entry.id != schema_id(Uuid::from_u128(33)))
    );
    assert!(
        operations::get_tags::run(&mut core)
            .unwrap()
            .tags
            .iter()
            .all(|tag| tag.name != "template-only")
    );
}

#[tokio::test]
async fn search_entries_preserves_relevance_and_applies_visibility_and_protection_policy() {
    let (mut core, ids) = query_core().await;
    let key = core.credential.as_ref().unwrap().restore_key().unwrap();
    let high_score_id = NodeId::from_uuid(Uuid::from_u128(60));
    let low_score_id = NodeId::from_uuid(Uuid::from_u128(61));
    let template_id = NodeId::from_uuid(Uuid::from_u128(62));
    let templates_uuid = Uuid::from_u128(63);
    {
        let database = core.handle.as_mut().unwrap().database_mut();
        let root_id = NodeId::from_uuid(ids.root_group);

        let mut high_score = Entry::new(high_score_id);
        high_score.set_title("Needle account");
        high_score.set_username(ProtectedString::new_plain("needle-user"));
        assert!(database.add_entry(high_score, &root_id));

        let mut low_score = Entry::new(low_score_id);
        low_score.set_title("Other account");
        low_score.set_username(ProtectedString::new_plain("needle-user"));
        assert!(database.add_entry(low_score, &root_id));

        let recycle_bin_id = database.create_recycle_bin();
        let mut trash = Entry::new(NodeId::from_uuid(Uuid::from_u128(64)));
        trash.set_title("Needle trash");
        assert!(database.add_entry(trash, &recycle_bin_id));

        let templates_id = NodeId::from_uuid(templates_uuid);
        assert!(database.add_group(Group::new(templates_id), &root_id));
        let mut template = Entry::new(template_id);
        template.set_title("Needle template");
        template.add_custom_field("_etm_template", ProtectedString::new_plain("1"));
        assert!(database.add_entry(template, &templates_id));
        database.entry_templates_uuid = Some(templates_uuid);
        database.protect_entry_strings(&key).unwrap();
    }

    let entries = operations::search_entries::run(
        &mut core,
        SearchEntriesArgs {
            query: "needle".into(),
        },
    )
    .unwrap()
    .entries;
    assert_eq!(
        entries
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec![
            crate::model::node_id(high_score_id),
            crate::model::node_id(low_score_id)
        ]
    );

    let protected = operations::execute(
        &mut core,
        Operation::SearchEntries(SearchEntriesArgs {
            query: "protected-title-secret".into(),
        }),
    )
    .await
    .unwrap();
    let OperationSuccess::SearchEntries(protected) = protected else {
        panic!("unexpected operation result");
    };
    assert_eq!(protected.entries.len(), 1);
    assert_eq!(protected.entries[0].id, schema_id(ids.child_entry));
    assert_eq!(protected.entries[0].name, None);

    assert!(
        operations::search_entries::run(
            &mut core,
            SearchEntriesArgs {
                query: "password-secret".into(),
            },
        )
        .unwrap()
        .entries
        .is_empty()
    );

    operations::set_config::run(
        &mut core,
        KeelessConfigPatch {
            auto_lock_timeout_ms: None,
            paranoia_mode: Some(true),
        },
    )
    .await
    .unwrap();
    assert!(core.credential.is_none());
    assert!(
        operations::search_entries::run(
            &mut core,
            SearchEntriesArgs {
                query: "protected-title-secret".into(),
            },
        )
        .unwrap()
        .entries
        .is_empty()
    );
}

#[tokio::test]
async fn search_fuzzy_applies_direct_group_and_tag_filters() {
    let (mut core, ids) = query_core().await;

    let result = operations::search_fuzzy::run(
        &mut core,
        SearchFuzzyArgs {
            query: "in:Child tag:shared".into(),
        },
    )
    .unwrap();

    assert_eq!(result.entries.len(), 1);
    assert_eq!(result.entries[0].id, schema_id(ids.child_entry));
    assert_eq!(result.entries[0].name, None);
    assert_eq!(result.filter_tokens, vec!["in:Child", "tag:shared"]);
}

#[tokio::test]
async fn search_fuzzy_matches_the_recycle_bin_title() {
    let (mut core, _) = query_core().await;
    {
        let database = core.handle.as_mut().unwrap().database_mut();
        let recycle_bin_id = database.create_recycle_bin();
        database.get_group_mut(&recycle_bin_id).unwrap().title = "Discarded Items".into();
    }

    let result = operations::search_fuzzy::run(
        &mut core,
        SearchFuzzyArgs {
            query: "discarded".into(),
        },
    )
    .unwrap();
    assert!(result.trash_matches);

    let result = operations::search_fuzzy::run(
        &mut core,
        SearchFuzzyArgs {
            query: "trash".into(),
        },
    )
    .unwrap();
    assert!(!result.trash_matches);
}
