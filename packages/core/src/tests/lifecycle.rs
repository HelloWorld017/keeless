use super::*;

#[tokio::test]
async fn state_lifecycle_failed_unlock_auto_lock_and_paranoia_sync() {
    let config = Arc::new(MemoryConfig::default());
    let clock = Arc::new(FakeClock::new(100));
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(database_bytes(b"correct")))));
    let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
    providers.insert("memory".into(), storage);
    let mut core = KeelessCore::new(KeelessHost {
        storage_providers: providers,
        ..host(config, Arc::new(Approval), clock.clone())
    })
    .await
    .unwrap();
    assert_eq!(
        operations::get_database_status::run(&mut core),
        DatabaseStatus::NotExist
    );
    assert_eq!(operations::get_storage_descriptor::run(&mut core), None);
    let descriptor = StorageDescriptor {
        provider: "memory".into(),
        path: "vault.kdbx".into(),
    };
    operations::open::run(&mut core, descriptor.clone())
        .await
        .unwrap();
    assert_eq!(
        operations::get_storage_descriptor::run(&mut core),
        Some(descriptor)
    );
    assert_eq!(
        operations::get_database_status::run(&mut core),
        DatabaseStatus::Locked
    );
    assert!(matches!(
        operations::unlock::run(&mut core, b"wrong").await,
        Err(CoreError::InvalidCredentials)
    ));
    assert_eq!(
        operations::get_database_status::run(&mut core),
        DatabaseStatus::Locked
    );
    operations::unlock::run(&mut core, b"correct")
        .await
        .unwrap();
    assert_eq!(
        operations::get_database_status::run(&mut core),
        DatabaseStatus::Unlocked
    );

    operations::set_config::run(
        &mut core,
        KeelessConfigPatch {
            auto_lock_timeout_ms: Some(Some(10)),
            paranoia_mode: Some(true),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        core.sync(None).await,
        Err(CoreError::PasswordRequired)
    ));
    assert!(matches!(
        core.sync(Some(b"wrong")).await,
        Err(CoreError::InvalidCredentials)
    ));
    operations::save_database::run(&mut core, Some(b"correct"))
        .await
        .unwrap();
    operations::set_config::run(
        &mut core,
        KeelessConfigPatch {
            auto_lock_timeout_ms: None,
            paranoia_mode: Some(false),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        operations::get_database_status::run(&mut core),
        DatabaseStatus::Locked
    );
    operations::unlock::run(&mut core, b"correct")
        .await
        .unwrap();
    clock.set(110);
    core.tick().await;
    assert_eq!(
        operations::get_database_status::run(&mut core),
        DatabaseStatus::Locked
    );
    operations::lock::run(&mut core);
    assert_eq!(
        operations::get_database_status::run(&mut core),
        DatabaseStatus::Locked
    );
}

#[tokio::test]
async fn password_provider_handles_missing_input_and_supplied_passwords_win() {
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(database_bytes(b"correct")))));
    let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
    providers.insert("memory".into(), storage);
    let mut core = KeelessCore::new(KeelessHost {
        storage_providers: providers,
        ..host(
            Arc::new(MemoryConfig::default()),
            Arc::new(Approval),
            Arc::new(FakeClock::new(100)),
        )
    })
    .await
    .unwrap();
    operations::open::run(
        &mut core,
        StorageDescriptor {
            provider: "memory".into(),
            path: "vault.kdbx".into(),
        },
    )
    .await
    .unwrap();

    assert!(matches!(
        operations::execute(
            &mut core,
            Operation::Unlock(keeless_schema::UnlockArgs { password: None }),
        )
        .await,
        Err(CoreError::PasswordRequired)
    ));

    let cancelled = Arc::new(PasswordInput::cancelled());
    core.password_input = Some(cancelled.clone());
    assert!(matches!(
        operations::execute(
            &mut core,
            Operation::Unlock(keeless_schema::UnlockArgs { password: None }),
        )
        .await,
        Err(CoreError::PasswordRequired)
    ));
    assert_eq!(
        cancelled.modes.lock().unwrap().as_slice(),
        &[PasswordInputMode::Unlock]
    );

    let input = Arc::new(PasswordInput::new(b"correct"));
    core.password_input = Some(input.clone());
    assert!(matches!(
        operations::execute(
            &mut core,
            Operation::Unlock(keeless_schema::UnlockArgs {
                password: Some("wrong".into())
            }),
        )
        .await,
        Err(CoreError::InvalidCredentials)
    ));
    assert!(input.modes.lock().unwrap().is_empty());

    operations::execute(
        &mut core,
        Operation::Unlock(keeless_schema::UnlockArgs { password: None }),
    )
    .await
    .unwrap();
    assert_eq!(
        input.modes.lock().unwrap().as_slice(),
        &[PasswordInputMode::Unlock]
    );
}

#[tokio::test]
async fn password_provider_receives_create_reveal_and_save_modes() {
    let create_input = Arc::new(PasswordInput::new(b"correct"));
    let storage = Arc::new(MemoryStorage(Mutex::new(None)));
    let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
    providers.insert("memory".into(), storage);
    let mut create_host = host(
        Arc::new(MemoryConfig::default()),
        Arc::new(Approval),
        Arc::new(FakeClock::new(100)),
    );
    create_host.storage_providers = providers;
    create_host.password_input = Some(create_input.clone());
    let mut create_core = KeelessCore::new(create_host).await.unwrap();
    operations::open::run(
        &mut create_core,
        StorageDescriptor {
            provider: "memory".into(),
            path: "new.kdbx".into(),
        },
    )
    .await
    .unwrap();
    operations::execute(
        &mut create_core,
        Operation::Create(keeless_schema::CreateArgs { password: None }),
    )
    .await
    .unwrap();
    assert_eq!(
        create_input.modes.lock().unwrap().as_slice(),
        &[PasswordInputMode::Create]
    );

    let (mut core, ids) = query_core().await;
    let input = Arc::new(PasswordInput::new(b"correct"));
    core.password_input = Some(input.clone());
    let entry_id = schema_id(ids.root_entry);
    let detail = operations::get_entry_detail::run(
        &mut core,
        GetEntryDetailArgs {
            entry_id: entry_id.clone(),
        },
    )
    .unwrap();
    let password_id = detail
        .fields
        .iter()
        .filter_map(detail_field)
        .find(|field| field.name == "Password")
        .unwrap()
        .field_id
        .unwrap()
        .to_owned();
    let fields = detail
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
    operations::reveal_entry_fields::run(
        &mut core,
        entry_id.clone(),
        vec![password_id.clone()],
        None,
    )
    .await
    .unwrap();
    assert!(input.modes.lock().unwrap().is_empty());
    operations::set_config::run(
        &mut core,
        KeelessConfigPatch {
            auto_lock_timeout_ms: None,
            paranoia_mode: Some(true),
        },
    )
    .await
    .unwrap();

    operations::reveal_entry_fields::run(&mut core, entry_id.clone(), vec![password_id], None)
        .await
        .unwrap();
    let _ = core.sync(None).await;
    operations::mutations::update_entry::run(&mut core, entry_id, fields, None, None)
        .await
        .unwrap();
    assert_eq!(
        input.modes.lock().unwrap().as_slice(),
        &[
            PasswordInputMode::Reveal,
            PasswordInputMode::Save,
            PasswordInputMode::Save,
        ]
    );
}

#[tokio::test]
async fn create_builds_and_unlocks_a_new_database_without_overwriting() {
    let storage = Arc::new(MemoryStorage(Mutex::new(None)));
    let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
    providers.insert("memory".into(), storage.clone());
    let mut core = KeelessCore::new(KeelessHost {
        storage_providers: providers,
        ..host(
            Arc::new(MemoryConfig::default()),
            Arc::new(Approval),
            Arc::new(FakeClock::new(100)),
        )
    })
    .await
    .unwrap();

    assert!(matches!(
        operations::create::run(&mut core, b"secret").await,
        Err(CoreError::NoDatabaseSelected)
    ));
    operations::open::run(
        &mut core,
        StorageDescriptor {
            provider: "memory".into(),
            path: "vault.kdbx".into(),
        },
    )
    .await
    .unwrap();
    operations::create::run(&mut core, b"secret").await.unwrap();
    assert_eq!(
        operations::get_database_status::run(&mut core),
        DatabaseStatus::Unlocked
    );
    let templates = operations::get_entry_templates::run(&mut core)
        .unwrap()
        .entries;
    assert_eq!(
        templates
            .iter()
            .map(|template| template.name.as_deref().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "General",
            "Credit Card",
            "Email Account",
            "Wireless Router",
            "Bank Account",
            "Secure Note",
            "SSH Key",
            "Membership",
        ]
    );
    assert!(
        operations::get_entries::run(&mut core, GetEntriesArgs::default())
            .unwrap()
            .entries
            .is_empty()
    );
    {
        let database = core.handle.as_ref().unwrap().database();
        let templates_group_id = NodeId::from_uuid(database.entry_templates_uuid.unwrap());
        let templates_group = database.get_group(&templates_group_id).unwrap();
        assert_eq!(templates_group.title, "Templates");
        assert!(!templates_group.enable_searching);
        assert!(
            database
                .root_group()
                .unwrap()
                .child_group_ids
                .contains(&templates_group_id)
        );
    }

    operations::lock::run(&mut core);
    assert!(matches!(
        operations::unlock::run(&mut core, b"wrong").await,
        Err(CoreError::InvalidCredentials)
    ));
    operations::unlock::run(&mut core, b"secret").await.unwrap();

    operations::lock::run(&mut core);
    assert!(matches!(
        operations::create::run(&mut core, b"replacement").await,
        Err(CoreError::DatabaseAlreadyExists)
    ));
    assert_eq!(
        operations::get_database_status::run(&mut core),
        DatabaseStatus::Locked
    );
    assert!(storage.0.lock().unwrap().is_some());
}

#[tokio::test]
async fn failed_reunlock_preserves_an_existing_unlocked_handle() {
    let clock = Arc::new(FakeClock::new(100));
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(database_bytes(b"correct")))));
    let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
    providers.insert("memory".into(), storage.clone());
    let mut core = KeelessCore::new(KeelessHost {
        storage_providers: providers,
        ..host(Arc::new(MemoryConfig::default()), Arc::new(Approval), clock)
    })
    .await
    .unwrap();
    operations::open::run(
        &mut core,
        StorageDescriptor {
            provider: "memory".into(),
            path: "vault.kdbx".into(),
        },
    )
    .await
    .unwrap();
    operations::unlock::run(&mut core, b"correct")
        .await
        .unwrap();
    *storage.0.lock().unwrap() = None;

    assert!(matches!(
        operations::unlock::run(&mut core, b"correct").await,
        Err(CoreError::DatabaseNotFound)
    ));
    assert_eq!(
        operations::get_database_status::run(&mut core),
        DatabaseStatus::Unlocked
    );
}
