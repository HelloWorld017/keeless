use super::*;
use keeless_kdbx::{
    CompositeCredentials, RekeyOptions, open_database_with_key, rekey_database, save_database,
};

// Unlike MemoryStorage, this fixture supports revision-checked dirty uploads.
struct RevisionStorage(Arc<MemoryStorage>, AtomicBool);

fn revision(bytes: &[u8]) -> keeless_sync::Revision {
    use sha2::{Digest, Sha256};
    keeless_sync::Revision::StrongEtag(format!("\"{:x}\"", Sha256::digest(bytes)))
}

impl StorageProvider for RevisionStorage {
    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<ByteRange>,
    ) -> StorageFuture<'a, std::result::Result<RemoteFile, StorageError>> {
        Box::pin(async move {
            let mut remote = self.0.read(path, range).await?;
            remote.metadata.revision = Some(revision(&remote.bytes));
            Ok(remote)
        })
    }

    fn stat<'a>(
        &'a self,
        path: &'a str,
    ) -> StorageFuture<'a, std::result::Result<Option<FileMetadata>, StorageError>> {
        Box::pin(async move {
            self.read(path, None)
                .await
                .map(|remote| Some(remote.metadata))
        })
    }

    fn write<'a>(
        &'a self,
        _: &'a str,
        bytes: Vec<u8>,
        condition: WriteCondition,
    ) -> StorageFuture<'a, std::result::Result<WriteOutcome, StorageError>> {
        Box::pin(async move {
            if self.1.load(Ordering::Relaxed) {
                return Err(StorageError::new(
                    StorageErrorKind::Other,
                    "injected write failure",
                ));
            }
            let mut current = self.0.0.lock().unwrap();
            let matches = match condition {
                WriteCondition::MustMatch(expected) => current
                    .as_ref()
                    .is_some_and(|bytes| revision(bytes) == expected),
                WriteCondition::MustNotExist => current.is_none(),
                _ => true,
            };
            if !matches {
                return Ok(WriteOutcome::Conflict);
            }
            let revision = Some(revision(&bytes));
            *current = Some(bytes);
            Ok(WriteOutcome::Applied { revision })
        })
    }

    fn delete<'a>(
        &'a self,
        path: &'a str,
    ) -> StorageFuture<'a, std::result::Result<(), StorageError>> {
        self.0.delete(path)
    }
}

#[tokio::test]
async fn prepared_rotation_retains_keys_on_write_failure_and_commits_only_after_migration() {
    let (bytes, _) = query_database_bytes(b"correct");
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(bytes))));
    let provider = Arc::new(RevisionStorage(storage.clone(), AtomicBool::new(false)));
    let persistence = Arc::new(MemoryDatabasePersistence::default());
    let (mut core, ids) = query_core_with_persistence(provider.clone(), persistence.clone()).await;
    let old_key = core.credential.as_ref().unwrap().restore_key().unwrap();
    let (parameters, next_key) = core
        .credential
        .as_ref()
        .unwrap()
        .restore_next()
        .unwrap()
        .unwrap();
    core.sync(None).await.unwrap();
    assert_eq!(
        core.credential
            .as_ref()
            .unwrap()
            .restore_next()
            .unwrap()
            .unwrap()
            .0,
        parameters
    );

    let added = operations::mutations::add_entry::run(
        &mut core,
        AddEntryArgs {
            parent_group_id: schema_id(ids.root_group),
        },
    )
    .await
    .unwrap();
    let added_id = model_id(added.id);
    let mut remote = open_database_with_key(
        storage.0.lock().unwrap().as_ref().unwrap().as_slice(),
        &old_key,
    )
    .unwrap();
    remote.name = "Concurrent remote edit".into();
    let mut remote_bytes = Vec::new();
    save_database(&mut remote_bytes, &remote, &old_key).unwrap();
    *storage.0.lock().unwrap() = Some(remote_bytes.clone());
    let checkpoint = core.handle.as_ref().unwrap().checkpoint_bytes().to_vec();
    let journal = persistence.journal.lock().unwrap().clone();

    provider.1.store(true, Ordering::Relaxed);
    assert!(core.rotate_kdf(None).await.is_err());
    assert!(core.rotate_kdf(Some(b"correct")).await.is_err());
    assert!(matches!(
        core.rotate_kdf(Some(b"wrong")).await,
        Err(CoreError::InvalidCredentials)
    ));
    assert_eq!(*storage.0.lock().unwrap(), Some(remote_bytes));
    assert_eq!(core.handle.as_ref().unwrap().checkpoint_bytes(), checkpoint);
    assert_eq!(*persistence.journal.lock().unwrap(), journal);
    assert_eq!(
        core.credential
            .as_ref()
            .unwrap()
            .restore_key()
            .unwrap()
            .kdf_fingerprint(),
        old_key.kdf_fingerprint()
    );
    assert_eq!(
        core.credential
            .as_ref()
            .unwrap()
            .restore_next()
            .unwrap()
            .unwrap()
            .0,
        parameters
    );

    provider.1.store(false, Ordering::Relaxed);
    persistence.fail_state_write.store(true, Ordering::Relaxed);
    assert!(matches!(
        core.rotate_kdf(None).await,
        Err(CoreError::Host(_))
    ));
    assert!(core.sync_key_migration_pending());
    let written = storage.0.lock().unwrap().clone().unwrap();
    let opened = open_database_with_key(written.as_slice(), &next_key).unwrap();
    assert!(opened.get_entry(&added_id).is_some());
    assert_eq!(opened.name, "Concurrent remote edit");
    assert_eq!(opened.kdf_parameters, Some(parameters.clone()));
    assert!(open_database_with_key(written.as_slice(), &old_key).is_err());

    persistence.fail_state_write.store(false, Ordering::Relaxed);
    // Retry only completes the in-flight migration; it must not rotate a second time.
    core.rotate_kdf(None).await.unwrap();
    assert!(!core.sync_key_migration_pending());
    assert_eq!(*storage.0.lock().unwrap(), Some(written));
    let vault = core.credential.as_ref().unwrap();
    assert_eq!(
        vault.restore_key().unwrap().kdf_fingerprint(),
        next_key.kdf_fingerprint()
    );
    assert!(vault.restore_next().unwrap().is_none());
    assert!(persistence.journal.lock().unwrap().is_empty());
    assert!(matches!(
        core.rotate_kdf(None).await,
        Err(CoreError::PasswordRequired)
    ));
    core.sync(None).await.unwrap();
    assert!(
        core.credential
            .as_ref()
            .unwrap()
            .restore_next()
            .unwrap()
            .is_none()
    );

    // Reauthentication replenishes the slot; password-free rotation is available again.
    core.sync(Some(b"correct")).await.unwrap();
    assert!(
        core.credential
            .as_ref()
            .unwrap()
            .restore_next()
            .unwrap()
            .is_some()
    );
    operations::lock::run(&mut core);
    assert!(core.credential.is_none());
    operations::unlock::run(&mut core, b"correct")
        .await
        .unwrap();
    assert_eq!(
        core.credential
            .as_ref()
            .unwrap()
            .restore_key()
            .unwrap()
            .kdf_fingerprint(),
        next_key.kdf_fingerprint()
    );
    assert!(
        core.credential
            .as_ref()
            .unwrap()
            .restore_next()
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn paranoid_rotation_derives_transient_keys_and_never_retains_a_vault() {
    let (bytes, _) = query_database_bytes(b"correct");
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(bytes))));
    let provider = Arc::new(RevisionStorage(storage.clone(), AtomicBool::new(false)));
    let persistence = Arc::new(MemoryDatabasePersistence::default());
    let (mut core, _) = query_core_with_persistence(provider, persistence).await;
    let old_fingerprint = core
        .credential
        .as_ref()
        .unwrap()
        .restore_key()
        .unwrap()
        .kdf_fingerprint();
    operations::set_config::run(
        &mut core,
        KeelessConfigPatch {
            paranoia_mode: Some(true),
            auto_lock_timeout_ms: None,
        },
    )
    .await
    .unwrap();
    assert!(core.credential.is_none());
    assert!(matches!(
        core.rotate_kdf(None).await,
        Err(CoreError::PasswordRequired)
    ));
    core.rotate_kdf(Some(b"correct")).await.unwrap();
    assert!(core.credential.is_none());
    let credentials = CompositeCredentials::new()
        .with_password(b"correct")
        .unwrap();
    let opened = keeless_kdbx::open_database(
        storage.0.lock().unwrap().as_ref().unwrap().as_slice(),
        &credentials,
    )
    .unwrap();
    assert_ne!(opened.key.kdf_fingerprint(), old_fingerprint);
}

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
async fn native_kdf_reauthentication_preserves_dirty_changes_on_failure_and_cancellation() {
    let (bytes, _) = query_database_bytes(b"correct");
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(bytes))));
    let persistence = Arc::new(MemoryDatabasePersistence::default());
    let (mut core, _) = query_core_with_persistence(
        Arc::new(RevisionStorage(storage.clone(), AtomicBool::new(false))),
        persistence.clone(),
    )
    .await;
    let parent_group_id = core
        .handle
        .as_ref()
        .unwrap()
        .database()
        .root_group_id
        .unwrap();
    let parent_group_id = match parent_group_id {
        NodeId::Uuid(id) => schema_id(id),
        NodeId::Int(id) => DatabaseNodeId::Int(id),
    };
    let added = operations::mutations::add_entry::run(&mut core, AddEntryArgs { parent_group_id })
        .await
        .unwrap();
    let added_id = model_id(added.id);
    let old_key = core.credential.as_ref().unwrap().restore_key().unwrap();
    let credentials = CompositeCredentials::new()
        .with_password(b"new-password")
        .unwrap();
    let mut remote = open_database_with_key(
        storage.0.lock().unwrap().as_ref().unwrap().as_slice(),
        &old_key,
    )
    .unwrap();
    remote.name = "Remote change".into();
    let remote_key =
        rekey_database(&mut remote, &old_key, &credentials, RekeyOptions::default()).unwrap();
    let mut remote_bytes = Vec::new();
    save_database(&mut remote_bytes, &remote, &remote_key).unwrap();
    *storage.0.lock().unwrap() = Some(remote_bytes.clone());
    let journal = persistence.journal.lock().unwrap().clone();
    let cache = persistence.cache.lock().unwrap().clone();

    let input = Arc::new(PasswordInput::new(b"new-password"));
    core.password_input = Some(input.clone());
    core.tick().await;
    core.tick().await;
    assert_eq!(core.sync_status(), SyncStatus::CredentialsRequired);
    assert!(input.modes.lock().unwrap().is_empty());

    let cancelled = Arc::new(PasswordInput::cancelled());
    core.password_input = Some(cancelled.clone());
    assert!(matches!(
        core.sync(None).await,
        Err(CoreError::PasswordRequired)
    ));
    assert_eq!(core.sync_status(), SyncStatus::CredentialsRequired);
    assert_eq!(
        *cancelled.modes.lock().unwrap(),
        vec![PasswordInputMode::Save]
    );

    let wrong = Arc::new(PasswordInput::new(b"wrong"));
    core.password_input = Some(wrong.clone());
    assert!(matches!(
        core.sync(None).await,
        Err(CoreError::InvalidCredentials)
    ));
    assert_eq!(*wrong.modes.lock().unwrap(), vec![PasswordInputMode::Save]);
    assert_eq!(*storage.0.lock().unwrap(), Some(remote_bytes));
    assert_eq!(*persistence.journal.lock().unwrap(), journal);
    assert_eq!(*persistence.cache.lock().unwrap(), cache);
    assert!(core.handle.as_ref().unwrap().is_dirty());
    assert!(
        core.handle
            .as_ref()
            .unwrap()
            .database()
            .get_entry(&added_id)
            .is_some()
    );

    core.password_input = Some(input.clone());
    let report = core.sync(None).await.unwrap();
    assert!(report.uploaded && report.downloaded);
    assert_eq!(*input.modes.lock().unwrap(), vec![PasswordInputMode::Save]);
    assert_eq!(core.sync_status(), SyncStatus::Idle);
    assert!(persistence.journal.lock().unwrap().is_empty());
    let bytes = storage.0.lock().unwrap().clone().unwrap();
    let opened = keeless_kdbx::open_database(bytes.as_slice(), &credentials).unwrap();
    assert_eq!(opened.database.name, "Remote change");
    assert!(opened.database.get_entry(&added_id).is_some());
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
async fn explicit_remote_password_can_differ_from_the_cached_local_password() {
    let (bytes, _) = query_database_bytes(b"correct");
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(bytes))));
    let persistence = Arc::new(MemoryDatabasePersistence::default());
    let (mut core, _) = query_core_with_persistence(storage.clone(), persistence).await;
    let previous_next = core
        .credential
        .as_ref()
        .unwrap()
        .restore_next()
        .unwrap()
        .unwrap()
        .1
        .kdf_fingerprint();
    let old_key = core.credential.as_ref().unwrap().restore_key().unwrap();
    let credentials = CompositeCredentials::new()
        .with_password(b"new-password")
        .unwrap();
    let mut remote = open_database_with_key(
        storage.0.lock().unwrap().as_ref().unwrap().as_slice(),
        &old_key,
    )
    .unwrap();
    let key = rekey_database(&mut remote, &old_key, &credentials, RekeyOptions::default()).unwrap();
    let mut bytes = Vec::new();
    save_database(&mut bytes, &remote, &key).unwrap();
    *storage.0.lock().unwrap() = Some(bytes);
    assert!(matches!(
        core.sync(None).await,
        Err(CoreError::CredentialsRequired)
    ));
    assert!(matches!(
        core.sync(Some(b"wrong")).await,
        Err(CoreError::InvalidCredentials)
    ));
    core.sync(Some(b"new-password")).await.unwrap();
    let (parameters, next) = core
        .credential
        .as_ref()
        .unwrap()
        .restore_next()
        .unwrap()
        .unwrap();
    assert_ne!(next.kdf_fingerprint(), previous_next);
    let expected = credentials.derive_key(&parameters).unwrap();
    assert!(
        expected
            .with_key(|expected| next.with_key(|next| next == expected))
            .unwrap()
            .unwrap()
    );
    assert_eq!(core.sync_status(), SyncStatus::Idle);
    assert_eq!(
        core.credential
            .as_ref()
            .unwrap()
            .restore_key()
            .unwrap()
            .kdf_fingerprint(),
        key.kdf_fingerprint()
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
    assert!(
        core.credential
            .as_ref()
            .unwrap()
            .restore_next()
            .unwrap()
            .is_some()
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
