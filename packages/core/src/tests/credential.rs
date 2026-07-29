use super::*;

#[tokio::test]
async fn reveal_entry_fields_handles_ids_duplicates_and_credentials() {
    let (mut core, ids) = query_core().await;
    let entry_id = schema_id(ids.root_entry);
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
    let password_id = field_id("Password", 0);
    let notes_id = field_id("Notes", 0);
    let public_id = field_id("Public", 0);
    let duplicate_first_id = field_id("Duplicate", 0);
    let duplicate_second_id = field_id("Duplicate", 1);

    assert_eq!(
        operations::reveal_entry_fields::run(
            &mut core,
            entry_id.clone(),
            vec![password_id.clone(), duplicate_first_id, duplicate_second_id],
            None,
        )
        .await
        .unwrap()
        .values,
        ["password-secret", "duplicate-first", "duplicate-second"]
    );
    assert!(matches!(
        operations::reveal_entry_fields::run(&mut core, entry_id.clone(), vec![public_id], None)
            .await,
        Err(CoreError::InvalidEntryField)
    ));
    assert!(matches!(
        operations::reveal_entry_fields::run(
            &mut core,
            entry_id.clone(),
            vec!["invalid".into()],
            None,
        )
        .await,
        Err(CoreError::InvalidEntryField)
    ));
    assert_eq!(
        keeless_schema::OperationError::from(&CoreError::InvalidEntryField).code,
        "invalid_entry_field"
    );
    assert!(matches!(
        operations::reveal_entry_fields::run(
            &mut core,
            schema_id(Uuid::from_u128(999)),
            vec![password_id.clone()],
            None,
        )
        .await,
        Err(CoreError::EntryNotFound)
    ));

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
        operations::reveal_entry_fields::run(
            &mut core,
            entry_id.clone(),
            vec![password_id.clone()],
            None,
        )
        .await,
        Err(CoreError::PasswordRequired)
    ));
    assert!(matches!(
        operations::reveal_entry_fields::run(
            &mut core,
            entry_id.clone(),
            vec![password_id.clone()],
            Some(b"wrong"),
        )
        .await,
        Err(CoreError::InvalidCredentials)
    ));
    assert_eq!(
        operations::reveal_entry_fields::run(
            &mut core,
            entry_id.clone(),
            vec![notes_id],
            Some(b"correct")
        )
        .await
        .unwrap()
        .values,
        ["notes-secret"]
    );

    operations::lock::run(&mut core);
    assert!(matches!(
        operations::reveal_entry_fields::run(
            &mut core,
            DatabaseNodeId::Uuid("invalid".into()),
            vec![password_id],
            Some(b"correct"),
        )
        .await,
        Err(CoreError::DatabaseLocked)
    ));
}

#[tokio::test]
async fn database_query_operations_report_lookup_errors_and_require_unlock() {
    let (mut core, _) = query_core().await;
    let missing = schema_id(Uuid::from_u128(999));

    assert!(matches!(
        operations::get_group_entries::run(
            &mut core,
            GetGroupEntriesArgs {
                group_id: missing.clone(),
            },
        ),
        Err(CoreError::GroupNotFound)
    ));
    assert!(matches!(
        operations::get_entry_detail::run(&mut core, GetEntryDetailArgs { entry_id: missing },),
        Err(CoreError::EntryNotFound)
    ));
    assert!(matches!(
        operations::get_group_entries::run(
            &mut core,
            GetGroupEntriesArgs {
                group_id: DatabaseNodeId::Uuid("invalid".into()),
            },
        ),
        Err(CoreError::InvalidNodeId)
    ));

    operations::lock::run(&mut core);
    assert!(matches!(
        operations::get_entries::run(&mut core, GetEntriesArgs::default()),
        Err(CoreError::DatabaseLocked)
    ));
    assert!(matches!(
        operations::search_entries::run(
            &mut core,
            SearchEntriesArgs {
                query: "entry".into(),
            },
        ),
        Err(CoreError::DatabaseLocked)
    ));
    assert!(matches!(
        operations::get_group_hierarchy::run(&mut core),
        Err(CoreError::DatabaseLocked)
    ));
    assert!(matches!(
        operations::get_tags::run(&mut core),
        Err(CoreError::DatabaseLocked)
    ));
    assert!(matches!(
        operations::get_custom_icons::run(&mut core),
        Err(CoreError::DatabaseLocked)
    ));
    assert!(matches!(
        operations::get_group_entries::run(
            &mut core,
            GetGroupEntriesArgs {
                group_id: DatabaseNodeId::Uuid("invalid".into()),
            },
        ),
        Err(CoreError::DatabaseLocked)
    ));
    assert!(matches!(
        operations::get_entry_detail::run(
            &mut core,
            GetEntryDetailArgs {
                entry_id: DatabaseNodeId::Uuid("invalid".into()),
            },
        ),
        Err(CoreError::DatabaseLocked)
    ));
}
