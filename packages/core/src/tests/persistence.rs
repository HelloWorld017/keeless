use super::*;

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
