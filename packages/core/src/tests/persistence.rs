use super::*;
use keeless_kdbx::{
    CompositeCredentials, RekeyOptions, open_database_with_key, rekey_database, save_database,
};

#[tokio::test]
async fn mutation_journal_is_encrypted_atomic_replayable_and_sequenced() {
    let (bytes, _) = query_database_bytes(b"correct");
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(bytes.clone()))));
    let persistence = Arc::new(MemoryDatabasePersistence::default());
    let (mut core, ids) = query_core_with_persistence(storage.clone(), persistence.clone()).await;

    let first = operations::mutations::add_entry::run(
        &mut core,
        AddEntryArgs {
            parent_group_id: schema_id(ids.child_group),
        },
    )
    .await
    .unwrap();
    let first_id = model_id(first.id.clone());
    let second = operations::mutations::add_entry::run(
        &mut core,
        AddEntryArgs {
            parent_group_id: schema_id(ids.child_group),
        },
    )
    .await
    .unwrap();

    let lines = persistence.journal.lock().unwrap().clone();
    assert_eq!(lines.len(), 2);
    let id_text = |id: &DatabaseNodeId| match id {
        DatabaseNodeId::Uuid(value) => value.clone(),
        DatabaseNodeId::Int(value) => value.to_string(),
    };
    for (sequence, line) in lines.iter().enumerate() {
        let encoded = String::from_utf8(line.clone()).unwrap();
        assert!(!encoded.contains("add_entry"));
        assert!(!encoded.contains(&id_text(&first.id)));
        assert!(!encoded.contains(&id_text(&second.id)));
        let envelope: serde_json::Value = serde_json::from_slice(line).unwrap();
        assert_eq!(envelope["version"], 2);
        assert_eq!(envelope["sequence"], sequence as u64);
        assert!(
            envelope["nonce"]
                .as_str()
                .unwrap()
                .bytes()
                .all(|byte| byte != b'=')
        );
    }

    persistence.fail_append.store(true, Ordering::Relaxed);
    let before = core.handle.as_ref().unwrap().database().entry_count();
    assert!(matches!(
        operations::mutations::add_entry::run(
            &mut core,
            AddEntryArgs {
                parent_group_id: schema_id(ids.child_group),
            },
        )
        .await,
        Err(CoreError::Host(_))
    ));
    assert_eq!(
        core.handle.as_ref().unwrap().database().entry_count(),
        before
    );
    assert_eq!(persistence.journal.lock().unwrap().len(), 2);
    persistence.fail_append.store(false, Ordering::Relaxed);

    *storage.0.lock().unwrap() = None;
    let (replayed_dirty, _) =
        query_core_with_persistence(storage.clone(), persistence.clone()).await;
    assert!(replayed_dirty.handle.as_ref().unwrap().is_dirty());
    assert!(
        replayed_dirty
            .handle
            .as_ref()
            .unwrap()
            .database()
            .get_entry(&first_id)
            .is_some()
    );
    *storage.0.lock().unwrap() = Some(bytes);

    // Simulate a crash after the cache watermark was committed but before old journal
    // records were removed. Replay must skip the already-covered prefix.
    let key = core.credential.as_ref().unwrap().restore_key().unwrap();
    let mut cached_database = Vec::new();
    save_database(
        &mut cached_database,
        core.handle.as_ref().unwrap().database(),
        &key,
    )
    .unwrap();
    *persistence.cache.lock().unwrap() = Some(
        core.journal
            .as_ref()
            .unwrap()
            .encode_cache(
                &cached_database,
                core.handle
                    .as_ref()
                    .unwrap()
                    .database()
                    .kdf_parameters
                    .as_ref()
                    .unwrap(),
            )
            .unwrap(),
    );
    *storage.0.lock().unwrap() = None;
    let (mut replayed, _) = query_core_with_persistence(storage, persistence).await;
    let database = replayed.handle.as_ref().unwrap().database();
    assert!(database.get_entry(&first_id).is_some());
    assert!(database.get_entry(&model_id(second.id)).is_some());
    assert!(!replayed.handle.as_ref().unwrap().is_dirty());
    assert_eq!(replayed.sync_status(), SyncStatus::Syncing);
    replayed.tick().await;
    assert_eq!(replayed.sync_status(), SyncStatus::Error);
    assert!(replayed.sync_error().is_some());
    assert!(replayed.handle.is_some());
}

#[tokio::test]
async fn sync_recovers_from_remote_kdf_rotation_and_replaces_key_bound_state() {
    let (bytes, _) = query_database_bytes(b"correct");
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(bytes))));
    let persistence = Arc::new(MemoryDatabasePersistence::default());
    let (mut core, _) = query_core_with_persistence(storage.clone(), persistence.clone()).await;
    operations::set_config::run(
        &mut core,
        KeelessConfigPatch {
            auto_lock_timeout_ms: Some(Some(42)),
            paranoia_mode: None,
        },
    )
    .await
    .unwrap();

    let old_key = core.credential.as_ref().unwrap().restore_key().unwrap();
    let credentials = CompositeCredentials::new()
        .with_password(b"correct")
        .unwrap();
    let mut remote = open_database_with_key(
        storage.0.lock().unwrap().as_ref().unwrap().as_slice(),
        &old_key,
    )
    .unwrap();
    let remote_key =
        rekey_database(&mut remote, &old_key, &credentials, RekeyOptions::default()).unwrap();
    let mut remote_bytes = Vec::new();
    save_database(&mut remote_bytes, &remote, &remote_key).unwrap();
    *storage.0.lock().unwrap() = Some(remote_bytes);

    assert!(matches!(
        core.sync(None).await,
        Err(CoreError::CredentialsRequired)
    ));
    assert_eq!(core.sync_status(), SyncStatus::CredentialsRequired);
    assert_eq!(
        core.sync_error().unwrap().code,
        "database_credentials_required"
    );
    core.tick().await;
    assert_eq!(core.sync_status(), SyncStatus::CredentialsRequired);

    let report = core.sync(Some(b"correct")).await.unwrap();
    assert!(report.downloaded);
    assert_eq!(core.sync_status(), SyncStatus::Idle);
    assert!(persistence.journal.lock().unwrap().is_empty());
    let active_key = core.credential.as_ref().unwrap().restore_key().unwrap();
    assert_eq!(active_key.kdf_fingerprint(), remote_key.kdf_fingerprint());
    let parameters = operations::mutations::MutationCoordinator::cache_kdf_parameters(
        persistence.cache.lock().unwrap().as_ref().unwrap(),
    )
    .unwrap();
    assert!(active_key.matches(&parameters));

    operations::lock::run(&mut core);
    operations::unlock::run(&mut core, b"correct")
        .await
        .unwrap();
    assert_eq!(
        operations::get_config::run(&mut core).auto_lock_timeout_ms,
        Some(42)
    );
    assert_eq!(
        core.credential
            .as_ref()
            .unwrap()
            .restore_key()
            .unwrap()
            .kdf_fingerprint(),
        remote_key.kdf_fingerprint()
    );
}

#[tokio::test]
async fn kdf_rotation_retries_local_key_migration_after_persistence_failure() {
    let (bytes, _) = query_database_bytes(b"correct");
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(bytes))));
    let persistence = Arc::new(MemoryDatabasePersistence::default());
    let (mut core, _) = query_core_with_persistence(storage.clone(), persistence.clone()).await;
    let old_key = core.credential.as_ref().unwrap().restore_key().unwrap();
    let credentials = CompositeCredentials::new()
        .with_password(b"correct")
        .unwrap();
    let mut remote = open_database_with_key(
        storage.0.lock().unwrap().as_ref().unwrap().as_slice(),
        &old_key,
    )
    .unwrap();
    let remote_key =
        rekey_database(&mut remote, &old_key, &credentials, RekeyOptions::default()).unwrap();
    let mut remote_bytes = Vec::new();
    save_database(&mut remote_bytes, &remote, &remote_key).unwrap();
    *storage.0.lock().unwrap() = Some(remote_bytes);
    persistence.fail_state_write.store(true, Ordering::Relaxed);

    assert!(matches!(
        core.sync(Some(b"correct")).await,
        Err(CoreError::Host(_))
    ));
    assert_eq!(core.sync_status(), SyncStatus::Error);
    assert!(matches!(
        operations::set_config::run(
            &mut core,
            KeelessConfigPatch {
                auto_lock_timeout_ms: Some(Some(5)),
                paranoia_mode: None,
            },
        )
        .await,
        Err(CoreError::SyncRecoveryRequired)
    ));
    persistence.fail_state_write.store(false, Ordering::Relaxed);

    core.sync(None).await.unwrap();
    assert_eq!(core.sync_status(), SyncStatus::Idle);
    assert_eq!(
        core.credential
            .as_ref()
            .unwrap()
            .restore_key()
            .unwrap()
            .kdf_fingerprint(),
        remote_key.kdf_fingerprint()
    );
}

#[tokio::test]
async fn journal_recovery_reopens_remote_with_credentials_after_kdf_rotation() {
    let (bytes, _) = query_database_bytes(b"correct");
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(bytes))));
    let persistence = Arc::new(MemoryDatabasePersistence::default());
    let (mut core, _) = query_core_with_persistence(storage.clone(), persistence.clone()).await;
    let old_key = core.credential.as_ref().unwrap().restore_key().unwrap();
    let credentials = CompositeCredentials::new()
        .with_password(b"correct")
        .unwrap();
    let mut remote = open_database_with_key(
        storage.0.lock().unwrap().as_ref().unwrap().as_slice(),
        &old_key,
    )
    .unwrap();
    let remote_key =
        rekey_database(&mut remote, &old_key, &credentials, RekeyOptions::default()).unwrap();
    let mut remote_bytes = Vec::new();
    save_database(&mut remote_bytes, &remote, &remote_key).unwrap();
    *storage.0.lock().unwrap() = Some(remote_bytes);
    persistence
        .journal
        .lock()
        .unwrap()
        .push(b"invalid journal".to_vec());

    operations::lock::run(&mut core);
    operations::unlock::run(&mut core, b"correct")
        .await
        .unwrap();

    assert!(persistence.journal.lock().unwrap().is_empty());
    assert_eq!(
        core.credential
            .as_ref()
            .unwrap()
            .restore_key()
            .unwrap()
            .kdf_fingerprint(),
        remote_key.kdf_fingerprint()
    );
}
