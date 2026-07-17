use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeless_kdbx::{
    open_database, save_database, CompositeKey, Database, DatabaseVersion, DateInstant, Entry,
    Group, IconImageCustom, NodeId,
};
use keeless_sync::{
    ByteRange, FileHandle, FileMetadata, RemoteFile, RetryPolicy, Revision, StorageError,
    StorageErrorKind, StorageFuture, StorageProvider, SyncError, SyncOptions, WriteCondition,
    WriteOutcome,
};

#[derive(Default)]
struct MemoryState {
    files: HashMap<String, (Vec<u8>, u64)>,
    forced_conflicts: usize,
    delete_on_create_conflict: bool,
    writes: usize,
}

#[derive(Default)]
struct MemoryStorage {
    state: Mutex<MemoryState>,
}

impl MemoryStorage {
    fn put(&self, path: &str, bytes: Vec<u8>) {
        let mut state = self.state.lock().unwrap();
        let revision = state
            .files
            .get(path)
            .map_or(1, |(_, revision)| revision + 1);
        state.files.insert(path.to_string(), (bytes, revision));
    }

    fn bytes(&self, path: &str) -> Vec<u8> {
        self.state.lock().unwrap().files[path].0.clone()
    }

    fn force_conflicts(&self, count: usize) {
        self.state.lock().unwrap().forced_conflicts = count;
    }

    fn delete_on_create_conflict(&self) {
        self.state.lock().unwrap().delete_on_create_conflict = true;
    }

    fn writes(&self) -> usize {
        self.state.lock().unwrap().writes
    }
}

impl StorageProvider for MemoryStorage {
    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<ByteRange>,
    ) -> StorageFuture<'a, Result<RemoteFile, StorageError>> {
        Box::pin(async move {
            let state = self.state.lock().unwrap();
            let (bytes, revision) = state.files.get(path).ok_or_else(|| {
                StorageError::new(StorageErrorKind::NotFound, format!("missing {path}"))
            })?;
            let bytes = if let Some(range) = range {
                let start = range.start as usize;
                let end = (range.end as usize + 1).min(bytes.len());
                bytes[start.min(bytes.len())..end.max(start.min(bytes.len()))].to_vec()
            } else {
                bytes.clone()
            };
            Ok(RemoteFile {
                metadata: FileMetadata {
                    size: bytes.len() as u64,
                    revision: Some(Revision::StrongEtag(format!("\"{revision}\""))),
                    last_modified: None,
                },
                bytes,
            })
        })
    }

    fn stat<'a>(
        &'a self,
        path: &'a str,
    ) -> StorageFuture<'a, Result<Option<FileMetadata>, StorageError>> {
        Box::pin(async move {
            let state = self.state.lock().unwrap();
            Ok(state.files.get(path).map(|(bytes, revision)| FileMetadata {
                size: bytes.len() as u64,
                revision: Some(Revision::StrongEtag(format!("\"{revision}\""))),
                last_modified: None,
            }))
        })
    }

    fn write<'a>(
        &'a self,
        path: &'a str,
        bytes: Vec<u8>,
        condition: WriteCondition,
    ) -> StorageFuture<'a, Result<WriteOutcome, StorageError>> {
        Box::pin(async move {
            let mut state = self.state.lock().unwrap();
            state.writes += 1;
            if state.forced_conflicts > 0 {
                state.forced_conflicts -= 1;
                return Ok(WriteOutcome::Conflict);
            }
            if state.delete_on_create_conflict
                && matches!(&condition, WriteCondition::MustNotExist)
                && state.files.contains_key(path)
            {
                state.delete_on_create_conflict = false;
                state.files.remove(path);
                return Ok(WriteOutcome::Conflict);
            }

            let matches = match (&condition, state.files.get(path)) {
                (WriteCondition::Unconditional, _) => true,
                (WriteCondition::MustNotExist, None) => true,
                (WriteCondition::MustNotExist, Some(_)) => false,
                (WriteCondition::MustMatch(Revision::StrongEtag(expected)), Some((_, current))) => {
                    expected == &format!("\"{current}\"")
                }
                (WriteCondition::MustMatch(Revision::LastModified(_)), _) => false,
                (WriteCondition::MustMatch(_), None) => false,
            };
            if !matches {
                return Ok(WriteOutcome::Conflict);
            }

            let revision = state
                .files
                .get(path)
                .map_or(1, |(_, revision)| revision + 1);
            state.files.insert(path.to_string(), (bytes, revision));
            Ok(WriteOutcome::Applied {
                revision: Some(Revision::StrongEtag(format!("\"{revision}\""))),
            })
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> StorageFuture<'a, Result<(), StorageError>> {
        Box::pin(async move {
            self.state.lock().unwrap().files.remove(path);
            Ok(())
        })
    }

    fn ensure_directory<'a>(
        &'a self,
        _path: &'a str,
    ) -> StorageFuture<'a, Result<(), StorageError>> {
        Box::pin(async { Ok(()) })
    }

    fn list<'a>(&'a self, _path: &'a str) -> StorageFuture<'a, Result<Vec<String>, StorageError>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn read_directory<'a>(
        &'a self,
        _path: &'a str,
    ) -> StorageFuture<'a, Result<Vec<String>, StorageError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

fn options(max_retries: usize) -> SyncOptions {
    SyncOptions {
        retry_policy: RetryPolicy {
            max_retries,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        },
        ..SyncOptions::default()
    }
}

fn database_with_entry(title: &str) -> (Database, NodeId) {
    let mut database = Database::new(DatabaseVersion::KDBX4);
    let root_id = NodeId::new_uuid();
    let mut root = Group::new(root_id);
    root.title = "Root".to_string();
    database.groups.insert(root_id, root);
    database.root_group_id = Some(root_id);

    let entry_id = NodeId::new_uuid();
    let mut entry = Entry::new(entry_id);
    entry.title = title.to_string();
    entry.last_modification_time = DateInstant::EpochMillis(1);
    assert!(database.add_entry(entry, &root_id));
    (database, entry_id)
}

fn encode(database: &Database, key: &CompositeKey) -> Vec<u8> {
    let mut bytes = Vec::new();
    save_database(&mut bytes, database, key).unwrap();
    bytes
}

#[tokio::test]
async fn open_syncs_local_changes_with_cas() {
    let key = CompositeKey::new().with_password(b"test");
    let storage = Arc::new(MemoryStorage::default());
    let (database, entry_id) = database_with_entry("base");
    storage.put("vault.kdbx", encode(&database, &key));

    let provider: Arc<dyn StorageProvider> = storage.clone();
    let mut handle = FileHandle::open(provider, "vault.kdbx", &key, options(2))
        .await
        .unwrap();
    handle
        .database_mut()
        .entries
        .get_mut(&entry_id)
        .unwrap()
        .title = "local".to_string();

    let report = handle.sync(&key).await.unwrap();
    assert!(report.uploaded);
    assert!(!report.downloaded);
    assert_eq!(report.attempts, 1);

    let saved = open_database(storage.bytes("vault.kdbx").as_slice(), &key).unwrap();
    assert_eq!(saved.entries[&entry_id].title, "local");
}

#[tokio::test]
async fn sync_three_way_merges_independent_local_and_remote_changes() {
    let key = CompositeKey::new().with_password(b"test");
    let storage = Arc::new(MemoryStorage::default());
    let (mut database, first_id) = database_with_entry("first");
    let root_id = database.root_group_id.unwrap();
    let second_id = NodeId::new_uuid();
    let mut second = Entry::new(second_id);
    second.title = "second".to_string();
    second.last_modification_time = DateInstant::EpochMillis(1);
    assert!(database.add_entry(second, &root_id));
    storage.put("vault.kdbx", encode(&database, &key));

    let provider: Arc<dyn StorageProvider> = storage.clone();
    let mut handle = FileHandle::open(provider, "vault.kdbx", &key, options(2))
        .await
        .unwrap();
    let local = handle.database_mut().entries.get_mut(&first_id).unwrap();
    local.title = "local first".to_string();
    local.last_modification_time = DateInstant::EpochMillis(10);

    let remote_bytes = storage.bytes("vault.kdbx");
    let mut remote = open_database(remote_bytes.as_slice(), &key).unwrap();
    let remote_entry = remote.entries.get_mut(&second_id).unwrap();
    remote_entry.title = "remote second".to_string();
    remote_entry.last_modification_time = DateInstant::EpochMillis(20);
    storage.put("vault.kdbx", encode(&remote, &key));

    let report = handle.sync(&key).await.unwrap();
    assert!(report.uploaded);
    assert!(report.downloaded);
    assert_eq!(handle.database().entries[&first_id].title, "local first");
    assert_eq!(handle.database().entries[&second_id].title, "remote second");
}

#[tokio::test]
async fn sync_retries_from_the_original_local_snapshot() {
    let key = CompositeKey::new().with_password(b"test");
    let storage = Arc::new(MemoryStorage::default());
    let (database, entry_id) = database_with_entry("base");
    storage.put("vault.kdbx", encode(&database, &key));
    let provider: Arc<dyn StorageProvider> = storage.clone();
    let mut handle = FileHandle::open(provider, "vault.kdbx", &key, options(2))
        .await
        .unwrap();
    handle
        .database_mut()
        .entries
        .get_mut(&entry_id)
        .unwrap()
        .title = "local".to_string();
    storage.force_conflicts(1);

    let report = handle.sync(&key).await.unwrap();
    assert_eq!(report.attempts, 2);
    assert_eq!(storage.writes(), 2);
    assert_eq!(handle.database().entries[&entry_id].title, "local");
}

#[tokio::test]
async fn sync_pulls_remote_changes_without_rewriting_an_unmodified_local_file() {
    let key = CompositeKey::new().with_password(b"test");
    let storage = Arc::new(MemoryStorage::default());
    let (database, entry_id) = database_with_entry("base");
    storage.put("vault.kdbx", encode(&database, &key));
    let provider: Arc<dyn StorageProvider> = storage.clone();
    let mut handle = FileHandle::open(provider, "vault.kdbx", &key, options(2))
        .await
        .unwrap();

    let mut remote = open_database(storage.bytes("vault.kdbx").as_slice(), &key).unwrap();
    remote.entries.get_mut(&entry_id).unwrap().title = "remote".to_string();
    storage.put("vault.kdbx", encode(&remote, &key));

    let report = handle.sync(&key).await.unwrap();
    assert!(report.downloaded);
    assert!(!report.uploaded);
    assert_eq!(storage.writes(), 0);
    assert_eq!(handle.database().entries[&entry_id].title, "remote");
}

#[tokio::test]
async fn metadata_merging_pulls_one_sided_changes_and_keeps_local_conflicts() {
    let key = CompositeKey::new().with_password(b"test");
    let storage = Arc::new(MemoryStorage::default());
    let (mut database, entry_id) = database_with_entry("base");
    database.name = "base name".to_string();
    database.custom_data.set("base", "value");
    let icon_id = *entry_id.as_uuid().unwrap();
    database.custom_icons.insert(
        icon_id,
        IconImageCustom::new(icon_id, b"base icon".to_vec()),
    );
    storage.put("vault.kdbx", encode(&database, &key));
    let provider: Arc<dyn StorageProvider> = storage.clone();
    let mut handle = FileHandle::open(provider, "vault.kdbx", &key, options(2))
        .await
        .unwrap();

    handle
        .database_mut()
        .entries
        .get_mut(&entry_id)
        .unwrap()
        .title = "local entry".to_string();
    handle.database_mut().custom_data.set("local", "value");
    let mut remote = open_database(storage.bytes("vault.kdbx").as_slice(), &key).unwrap();
    remote.name = "remote name".to_string();
    remote.custom_data.set("remote", "value");
    remote.custom_icons.get_mut(&icon_id).unwrap().data = b"remote icon".to_vec();
    remote
        .kdf_parameters
        .as_mut()
        .unwrap()
        .set_uint32("X-Sync-Test", 7);
    storage.put("vault.kdbx", encode(&remote, &key));
    handle.sync(&key).await.unwrap();
    assert_eq!(handle.database().name, "remote name");
    assert_eq!(handle.database().custom_data.get("local"), Some("value"));
    assert_eq!(handle.database().custom_data.get("remote"), Some("value"));
    assert_eq!(
        handle.database().custom_icons[&icon_id].data,
        b"remote icon"
    );
    let persisted = open_database(storage.bytes("vault.kdbx").as_slice(), &key).unwrap();
    assert_eq!(
        handle.database().kdf_parameters,
        persisted.kdf_parameters,
        "in-memory KDF must match the representation written to the checkpoint"
    );

    handle.database_mut().name = "local conflict".to_string();
    handle.database_mut().custom_icons.remove(&icon_id);
    let mut remote = open_database(storage.bytes("vault.kdbx").as_slice(), &key).unwrap();
    remote.name = "remote conflict".to_string();
    remote.custom_icons.get_mut(&icon_id).unwrap().data = b"remote conflict icon".to_vec();
    storage.put("vault.kdbx", encode(&remote, &key));
    handle.sync(&key).await.unwrap();
    assert_eq!(handle.database().name, "local conflict");
    assert!(!handle.database().custom_icons.contains_key(&icon_id));
}

#[tokio::test]
async fn retry_exhaustion_preserves_the_local_database_and_checkpoint() {
    let key = CompositeKey::new().with_password(b"test");
    let storage = Arc::new(MemoryStorage::default());
    let (database, entry_id) = database_with_entry("base");
    storage.put("vault.kdbx", encode(&database, &key));
    let provider: Arc<dyn StorageProvider> = storage.clone();
    let mut handle = FileHandle::open(provider, "vault.kdbx", &key, options(1))
        .await
        .unwrap();
    let original_revision = handle.checkpoint_revision().cloned();
    handle
        .database_mut()
        .entries
        .get_mut(&entry_id)
        .unwrap()
        .title = "unsaved".to_string();
    storage.force_conflicts(2);

    let error = handle.sync(&key).await.unwrap_err();
    assert!(matches!(error, SyncError::RetryExhausted { attempts: 2 }));
    assert_eq!(handle.database().entries[&entry_id].title, "unsaved");
    assert_eq!(handle.checkpoint_revision(), original_revision.as_ref());
}

#[tokio::test]
async fn create_merges_when_the_remote_file_already_exists() {
    let key = CompositeKey::new().with_password(b"test");
    let storage = Arc::new(MemoryStorage::default());
    let (remote, remote_id) = database_with_entry("remote");
    storage.put("vault.kdbx", encode(&remote, &key));

    let (local, local_id) = database_with_entry("local");
    let provider: Arc<dyn StorageProvider> = storage.clone();
    let handle = FileHandle::create(provider, "vault.kdbx", local, &key, options(2))
        .await
        .unwrap();

    assert!(handle.database().entries.contains_key(&local_id));
    assert!(handle.database().entries.contains_key(&remote_id));
}

#[tokio::test]
async fn create_retries_when_a_conflicting_remote_is_deleted() {
    let key = CompositeKey::new().with_password(b"test");
    let storage = Arc::new(MemoryStorage::default());
    let (remote, _) = database_with_entry("remote");
    storage.put("vault.kdbx", encode(&remote, &key));
    storage.delete_on_create_conflict();

    let (local, local_id) = database_with_entry("local");
    let provider: Arc<dyn StorageProvider> = storage.clone();
    let handle = FileHandle::create(provider, "vault.kdbx", local, &key, options(2))
        .await
        .unwrap();

    assert!(handle.database().entries.contains_key(&local_id));
    assert_eq!(storage.writes(), 2);
}

#[tokio::test]
async fn file_handle_debug_does_not_expose_database_contents() {
    let key = CompositeKey::new().with_password(b"test");
    let storage = Arc::new(MemoryStorage::default());
    let (mut database, _) = database_with_entry("sensitive title");
    database.name = "sensitive database name".to_string();
    storage.put("vault.kdbx", encode(&database, &key));
    let provider: Arc<dyn StorageProvider> = storage;
    let handle = FileHandle::open(provider, "vault.kdbx", &key, options(0))
        .await
        .unwrap();

    let debug = format!("{handle:?}");
    assert!(!debug.contains("sensitive title"));
    assert!(!debug.contains("sensitive database name"));
}
