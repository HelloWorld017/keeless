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
async fn get_entry_totp_requires_a_protected_totp_field_and_valid_credentials() {
    let (mut core, ids) = query_core().await;
    let entry_id = schema_id(ids.root_entry);
    let key = core.credential.as_ref().unwrap().restore_key().unwrap();
    let entry = core
        .handle
        .as_mut()
        .unwrap()
        .database_mut()
        .get_entry_mut(&NodeId::from_uuid(ids.root_entry))
        .unwrap();
    entry.add_custom_field(
        "TOTP",
        ProtectedString::new_protected(
            "otpauth://totp/test?secret=JBSWY3DPEHPK3PXP&algorithm=SHA256&digits=8&period=30",
        ),
    );
    entry.add_custom_field(
        "HOTP",
        ProtectedString::new_protected("otpauth://hotp/test?secret=JBSWY3DPEHPK3PXP&counter=1"),
    );
    core.handle
        .as_mut()
        .unwrap()
        .database_mut()
        .protect_entry_strings(&key)
        .unwrap();
    let detail = operations::get_entry_detail::run(
        &mut core,
        GetEntryDetailArgs {
            entry_id: entry_id.clone(),
        },
    )
    .unwrap();
    let field_id = |name: &str| {
        detail
            .fields
            .iter()
            .filter_map(detail_field)
            .find(|field| field.name == name)
            .unwrap()
            .field_id
            .unwrap()
            .to_string()
    };

    let result =
        operations::get_entry_totp::run(&mut core, entry_id.clone(), Some(field_id("TOTP")), None)
            .await
            .unwrap();
    assert_eq!(result.digits, 8);
    assert_eq!(result.period, 30);
    assert_eq!(result.expires_at_ms, 30_000);
    assert_eq!(result.code.len(), 8);
    assert!(matches!(
        operations::get_entry_totp::run(
            &mut core,
            entry_id,
            Some(field_id("HOTP")),
            Some(b"wrong"),
        )
        .await,
        Err(CoreError::InvalidCredentials)
    ));
    assert!(matches!(
        operations::get_entry_totp::run(
            &mut core,
            schema_id(ids.root_entry),
            Some(field_id("HOTP")),
            None,
        )
        .await,
        Err(CoreError::InvalidTotp)
    ));
}

#[tokio::test]
async fn get_entry_totp_reads_keepass_timeotp_fields_without_exposing_them() {
    let (mut core, ids) = query_core().await;
    let entry_id = schema_id(ids.root_entry);
    let key = core.credential.as_ref().unwrap().restore_key().unwrap();
    let entry = core
        .handle
        .as_mut()
        .unwrap()
        .database_mut()
        .get_entry_mut(&NodeId::from_uuid(ids.root_entry))
        .unwrap();
    entry.add_custom_field(
        "TimeOtp-Secret-Base64",
        ProtectedString::new_protected("MTIzNDU2Nzg5MDEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU2Nzg5MDE="),
    );
    entry.add_custom_field(
        "TimeOtp-Algorithm",
        ProtectedString::new_plain("HMAC-SHA-256"),
    );
    entry.add_custom_field("TimeOtp-Length", ProtectedString::new_plain("8"));
    entry.add_custom_field("TimeOtp-Period", ProtectedString::new_plain("60"));
    core.handle
        .as_mut()
        .unwrap()
        .database_mut()
        .protect_entry_strings(&key)
        .unwrap();

    let detail = operations::get_entry_detail::run(
        &mut core,
        GetEntryDetailArgs {
            entry_id: entry_id.clone(),
        },
    )
    .unwrap();
    assert!(detail.fields.iter().any(|field| {
        matches!(
            field,
            EntryFieldInformation::TimeOtp { label, .. } if label == "OTP"
        )
    }));
    assert!(
        detail
            .fields
            .iter()
            .filter_map(detail_field)
            .filter(|field| field.name.starts_with("TimeOtp-"))
            .all(|field| field.is_internal)
    );

    let result = operations::get_entry_totp::run(&mut core, entry_id, None, None)
        .await
        .unwrap();
    assert_eq!(result.digits, 8);
    assert_eq!(result.period, 60);
    assert_eq!(result.expires_at_ms, 60_000);
    assert_eq!(result.code.len(), 8);
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
