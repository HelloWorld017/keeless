use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
};

use keeless_kdbx::kdbx::template;
use keeless_kdbx::{
    Database, DatabaseVersion, Entry, EntryBinary, EntryFieldSelector, Group, IconImageCustom,
    IconImageStandard, NodeId, ProtectedString, save_database,
};
use keeless_schema::{
    AddEntryArgs, AddEntryFromTemplateArgs, AddGroupArgs, DatabaseNodeId, DeleteEntryArgs,
    DeleteGroupArgs, DeleteTagArgs, EntryFieldInformation,
    EntryFieldUpdate as SchemaEntryFieldUpdate, EntryPropertiesUpdate, FieldControl,
    GetEntriesArgs, GetEntryDetailArgs, GetGroupEntriesArgs, GetTagEntriesArgs, IconReference,
    MoveEntryArgs, MoveGroupArgs, Operation, OperationSuccess, RenameGroupArgs, SearchEntriesArgs,
    TagStyle, UpdateGroupArgs, UpdateTagStyleArgs,
};
use keeless_sync::{
    ByteRange, FileMetadata, RemoteFile, StorageError, StorageErrorKind, StorageFuture,
    WriteCondition, WriteOutcome,
};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::*;

struct DetailField<'a> {
    order: u64,
    field_id: Option<&'a str>,
    kind: &'a keeless_schema::EntryFieldKind,
    name: &'a str,
    label: &'a str,
    value: Option<&'a str>,
    is_protected: bool,
    is_internal: bool,
    control: Option<&'a FieldControl>,
}

fn detail_field(field: &EntryFieldInformation) -> Option<DetailField<'_>> {
    let EntryFieldInformation::Field {
        order,
        field_id,
        kind,
        name,
        label,
        value,
        is_protected,
        is_internal,
        control,
    } = field
    else {
        return None;
    };
    Some(DetailField {
        order: *order,
        field_id: field_id.as_deref(),
        kind,
        name,
        label,
        value: value.as_deref(),
        is_protected: *is_protected,
        is_internal: *is_internal,
        control: control.as_ref(),
    })
}

#[derive(Default)]
struct MemoryConfig(Mutex<Option<Vec<u8>>>);

impl ConfigProvider for MemoryConfig {
    fn load(&self) -> HostFuture<'_, Result<Option<Vec<u8>>>> {
        Box::pin(async { Ok(self.0.lock().unwrap().clone()) })
    }

    fn save<'a>(&'a self, value: &'a [u8]) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            *self.0.lock().unwrap() = Some(value.to_vec());
            Ok(())
        })
    }
}

#[derive(Default)]
struct FailingConfig {
    value: Mutex<Option<Vec<u8>>>,
    fail_save: AtomicBool,
}

impl ConfigProvider for FailingConfig {
    fn load(&self) -> HostFuture<'_, Result<Option<Vec<u8>>>> {
        Box::pin(async { Ok(self.value.lock().unwrap().clone()) })
    }

    fn save<'a>(&'a self, value: &'a [u8]) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            if self.fail_save.load(Ordering::Relaxed) {
                return Err(CoreError::Host("save failed".into()));
            }
            *self.value.lock().unwrap() = Some(value.to_vec());
            Ok(())
        })
    }
}

#[derive(Default)]
struct MemoryDatabasePersistence {
    cache: Mutex<Option<Vec<u8>>>,
    journal: Mutex<Vec<Vec<u8>>>,
    selected: Mutex<Option<StorageDescriptor>>,
    fail_append: AtomicBool,
}

impl DatabasePersistence for MemoryDatabasePersistence {
    fn select<'a>(&'a self, descriptor: &'a StorageDescriptor) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            *self.selected.lock().unwrap() = Some(descriptor.clone());
            Ok(())
        })
    }

    fn identity(&self) -> HostFuture<'_, Result<Vec<u8>>> {
        Box::pin(async { Ok(b"test-persistence/vault.kdbx".to_vec()) })
    }

    fn read_cache(&self) -> HostFuture<'_, Result<Option<Vec<u8>>>> {
        Box::pin(async { Ok(self.cache.lock().unwrap().clone()) })
    }

    fn write_cache<'a>(&'a self, cache: &'a [u8]) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            *self.cache.lock().unwrap() = Some(cache.to_vec());
            Ok(())
        })
    }

    fn read_journal(&self) -> HostFuture<'_, Result<Vec<Vec<u8>>>> {
        Box::pin(async { Ok(self.journal.lock().unwrap().clone()) })
    }

    fn append_journal<'a>(&'a self, line: &'a [u8]) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            if self.fail_append.load(Ordering::Relaxed) {
                return Err(CoreError::Host("journal append failed".into()));
            }
            self.journal.lock().unwrap().push(line.to_vec());
            Ok(())
        })
    }

    fn clear_journal(&self) -> HostFuture<'_, Result<()>> {
        Box::pin(async {
            self.journal.lock().unwrap().clear();
            Ok(())
        })
    }

    fn quarantine_cache<'a>(&'a self, _reason: &'a str) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            *self.cache.lock().unwrap() = None;
            Ok(())
        })
    }

    fn quarantine_journal<'a>(&'a self, _reason: &'a str) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            self.journal.lock().unwrap().clear();
            Ok(())
        })
    }
}

struct Approval;

struct PasswordInput {
    password: Option<Vec<u8>>,
    modes: Mutex<Vec<PasswordInputMode>>,
}

impl PasswordInput {
    fn new(password: &[u8]) -> Self {
        Self {
            password: Some(password.to_vec()),
            modes: Mutex::new(Vec::new()),
        }
    }

    fn cancelled() -> Self {
        Self {
            password: None,
            modes: Mutex::new(Vec::new()),
        }
    }
}

impl PasswordInputProvider for PasswordInput {
    fn request_password(
        &self,
        mode: PasswordInputMode,
    ) -> HostFuture<'_, Result<Option<Zeroizing<Vec<u8>>>>> {
        self.modes.lock().unwrap().push(mode);
        Box::pin(async { Ok(self.password.clone().map(Zeroizing::new)) })
    }
}

struct FakeClock {
    wall: AtomicI64,
    monotonic: AtomicU64,
}

impl FakeClock {
    fn new(millis: u64) -> Self {
        Self {
            wall: AtomicI64::new(millis as i64),
            monotonic: AtomicU64::new(millis),
        }
    }

    fn set(&self, millis: u64) {
        self.wall.store(millis as i64, Ordering::Relaxed);
        self.monotonic.store(millis, Ordering::Relaxed);
    }
}

impl Clock for FakeClock {
    fn now_millis(&self) -> i64 {
        self.wall.load(Ordering::Relaxed)
    }

    fn monotonic_millis(&self) -> u64 {
        self.monotonic.load(Ordering::Relaxed)
    }
}

struct MemoryStorage(Mutex<Option<Vec<u8>>>);

impl StorageProvider for MemoryStorage {
    fn read<'a>(
        &'a self,
        _: &'a str,
        _: Option<ByteRange>,
    ) -> StorageFuture<'a, std::result::Result<RemoteFile, StorageError>> {
        Box::pin(async {
            let bytes = self
                .0
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| StorageError::new(StorageErrorKind::NotFound, "not found"))?;
            Ok(RemoteFile {
                metadata: metadata(&bytes),
                bytes,
            })
        })
    }

    fn stat<'a>(
        &'a self,
        _: &'a str,
    ) -> StorageFuture<'a, std::result::Result<Option<FileMetadata>, StorageError>> {
        Box::pin(async { Ok(self.0.lock().unwrap().as_ref().map(|bytes| metadata(bytes))) })
    }

    fn write<'a>(
        &'a self,
        _: &'a str,
        bytes: Vec<u8>,
        condition: WriteCondition,
    ) -> StorageFuture<'a, std::result::Result<WriteOutcome, StorageError>> {
        Box::pin(async move {
            let mut current = self.0.lock().unwrap();
            if condition == WriteCondition::MustNotExist && current.is_some() {
                return Ok(WriteOutcome::Conflict);
            }
            *current = Some(bytes);
            Ok(WriteOutcome::Applied { revision: None })
        })
    }

    fn delete<'a>(
        &'a self,
        _: &'a str,
    ) -> StorageFuture<'a, std::result::Result<(), StorageError>> {
        Box::pin(async {
            *self.0.lock().unwrap() = None;
            Ok(())
        })
    }
}

fn metadata(bytes: &[u8]) -> FileMetadata {
    FileMetadata {
        size: bytes.len() as u64,
        revision: None,
        last_modified: None,
    }
}

fn database_bytes(password: &[u8]) -> Vec<u8> {
    let mut database = Database::new(DatabaseVersion::KDBX4);
    let root_id = NodeId::new_uuid();
    let mut root = Group::new(root_id);
    root.title = "Root".into();
    database.groups.insert(root_id, root);
    database.root_group_id = Some(root_id);
    let key = CompositeKey::new().with_password(password).unwrap();
    let mut bytes = Vec::new();
    save_database(&mut bytes, &database, &key).unwrap();
    bytes
}

#[derive(Clone, Copy)]
struct QueryIds {
    root_group: Uuid,
    child_group: Uuid,
    nested_group: Uuid,
    root_entry: Uuid,
    child_entry: Uuid,
    nested_entry: Uuid,
}

fn query_database_bytes(password: &[u8]) -> (Vec<u8>, QueryIds) {
    let ids = QueryIds {
        root_group: Uuid::from_u128(1),
        child_group: Uuid::from_u128(2),
        nested_group: Uuid::from_u128(3),
        root_entry: Uuid::from_u128(10),
        child_entry: Uuid::from_u128(11),
        nested_entry: Uuid::from_u128(12),
    };
    let root_group_id = NodeId::from_uuid(ids.root_group);
    let child_group_id = NodeId::from_uuid(ids.child_group);
    let nested_group_id = NodeId::from_uuid(ids.nested_group);
    let root_entry_id = NodeId::from_uuid(ids.root_entry);
    let child_entry_id = NodeId::from_uuid(ids.child_entry);
    let nested_entry_id = NodeId::from_uuid(ids.nested_entry);

    let mut database = Database::new(DatabaseVersion::KDBX4);
    database.name = "Test Database".into();
    let mut root = Group::new(root_group_id);
    root.title = "Root".into();
    root.child_group_ids.push(child_group_id);
    root.child_entry_ids.push(root_entry_id);

    let custom_icon_uuid = Uuid::from_u128(100);
    let mut child = Group::new(child_group_id);
    child.title = "Child".into();
    child.icon = keeless_kdbx::IconImage::Standard(IconImageStandard::new(4));
    child.custom_icon_uuid = Some(custom_icon_uuid);
    child.child_group_ids.push(nested_group_id);
    child.child_entry_ids.push(child_entry_id);

    let mut nested = Group::new(nested_group_id);
    nested.title = "Nested".into();
    nested.child_entry_ids.push(nested_entry_id);

    database.groups.insert(root_group_id, root);
    database.groups.insert(child_group_id, child);
    database.groups.insert(nested_group_id, nested);
    database.root_group_id = Some(root_group_id);
    let mut custom_icon = IconImageCustom::new(custom_icon_uuid, vec![1, 2, 3]);
    custom_icon.name = "Custom".into();
    custom_icon.last_modification_time = 1_700_000_000_000;
    database.custom_icons.insert(custom_icon_uuid, custom_icon);

    let mut root_entry = Entry::new(root_entry_id);
    root_entry.set_title("Root Entry");
    root_entry.set_username(ProtectedString::new_plain("alice"));
    root_entry.set_password(ProtectedString::new_protected("password-secret"));
    root_entry.set_url("https://example.test");
    root_entry.set_notes(ProtectedString::new_protected("notes-secret"));
    root_entry.icon = keeless_kdbx::IconImage::Standard(IconImageStandard::new(7));
    root_entry.tags = vec!["shared".into(), "work".into(), "shared".into()];
    root_entry.background_color = "#000000".into();
    root_entry.foreground_color = "#ffffff".into();
    root_entry.override_url = "cmd://open".into();
    root_entry.usage_count = 3;
    root_entry.add_custom_field("Public", ProtectedString::new_plain("public-value"));
    root_entry.add_custom_field("Secret", ProtectedString::new_protected("custom-secret"));
    root_entry.binaries.push(EntryBinary {
        name: "secret.bin".into(),
        data: b"attachment-secret".to_vec(),
        is_protected: true,
    });

    let mut child_entry = Entry::new(child_entry_id);
    child_entry.set_title(ProtectedString::new_protected("protected-title-secret"));
    child_entry.set_username(ProtectedString::new_protected("protected-username-secret"));
    child_entry.set_url(ProtectedString::new_protected("protected-url-secret"));
    child_entry.tags = vec!["shared".into()];
    child_entry.custom_icon_uuid = Some(custom_icon_uuid);

    let mut nested_entry = Entry::new(nested_entry_id);
    nested_entry.set_title("Nested Entry");
    nested_entry.tags = vec!["nested".into()];

    database.entries.insert(root_entry_id, root_entry);
    database.entries.insert(child_entry_id, child_entry);
    database.entries.insert(nested_entry_id, nested_entry);

    let key = CompositeKey::new().with_password(password).unwrap();
    let mut bytes = Vec::new();
    save_database(&mut bytes, &database, &key).unwrap();
    (bytes, ids)
}

async fn query_core() -> (KeelessCore, QueryIds) {
    let (bytes, ids) = query_database_bytes(b"correct");
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(bytes))));
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
    operations::unlock::run(&mut core, b"correct")
        .await
        .unwrap();
    let key = core.credential.as_ref().unwrap().restore_key().unwrap();
    let database = core.handle.as_mut().unwrap().database_mut();
    let entry = database
        .get_entry_mut(&NodeId::from_uuid(ids.root_entry))
        .unwrap();
    entry.add_custom_field(
        "Duplicate",
        ProtectedString::new_protected("duplicate-first"),
    );
    entry.add_custom_field(
        "Duplicate",
        ProtectedString::new_protected("duplicate-second"),
    );
    database.protect_entry_strings(&key).unwrap();
    (core, ids)
}

async fn query_core_with_persistence(
    storage: Arc<MemoryStorage>,
    persistence: Arc<MemoryDatabasePersistence>,
) -> (KeelessCore, QueryIds) {
    let (_, ids) = query_database_bytes(b"correct");
    let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
    providers.insert("memory".into(), storage);
    let mut core = KeelessCore::new(KeelessHost {
        storage_providers: providers,
        config_provider: Arc::new(MemoryConfig::default()),
        password_input: None,
        clock: Arc::new(FakeClock::new(1234)),
        database_persistence: Some(persistence),
        task_spawner: None,
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
    (core, ids)
}

fn schema_id(id: Uuid) -> DatabaseNodeId {
    DatabaseNodeId::Uuid(id.hyphenated().to_string())
}

fn model_id(id: DatabaseNodeId) -> NodeId {
    match id {
        DatabaseNodeId::Uuid(value) => NodeId::from_uuid(Uuid::parse_str(&value).unwrap()),
        DatabaseNodeId::Int(value) => NodeId::from_int(value),
    }
}

fn host(
    config: Arc<dyn ConfigProvider>,
    _approval: Arc<Approval>,
    clock: Arc<FakeClock>,
) -> KeelessHost {
    KeelessHost {
        storage_providers: HashMap::new(),
        config_provider: config,
        password_input: None,
        clock,
        database_persistence: None,
        task_spawner: None,
    }
}

#[tokio::test]
async fn config_patch_is_deep_and_paranoia_is_persisted() {
    let config = Arc::new(MemoryConfig::default());
    let mut core = KeelessCore::new(host(
        config.clone(),
        Arc::new(Approval),
        Arc::new(FakeClock::new(0)),
    ))
    .await
    .unwrap();
    operations::set_config::run(
        &mut core,
        KeelessConfigPatch {
            auto_lock_timeout_ms: Some(Some(25)),
            paranoia_mode: None,
        },
    )
    .await
    .unwrap();
    operations::set_config::run(
        &mut core,
        KeelessConfigPatch {
            auto_lock_timeout_ms: None,
            paranoia_mode: Some(true),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        operations::get_config::run(&mut core).auto_lock_timeout_ms,
        Some(25)
    );
    assert!(operations::get_config::run(&mut core).paranoia_mode);
    let bytes = config.0.lock().unwrap().clone().unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["version"],
        1
    );
}

#[tokio::test]
async fn plaintext_payload_dispatches_and_rejects_invalid_requests() {
    let mut core = KeelessCore::new(host(
        Arc::new(MemoryConfig::default()),
        Arc::new(Approval),
        Arc::new(FakeClock::new(0)),
    ))
    .await
    .unwrap();
    let response = core
        .handle_payload(br#"{"requestId":"request-1","op":"getDatabaseStatus","args":{}}"#)
        .await
        .unwrap()
        .unwrap();
    let response: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(response["requestId"], "request-1");
    assert_eq!(response["status"], "success");
    assert_eq!(response["result"]["status"], "not_exist");
    assert!(core.handle_payload(b"not json").await.unwrap().is_none());
}

#[tokio::test]
async fn failed_persistence_rolls_back_settings() {
    let config = Arc::new(FailingConfig::default());
    let clock = Arc::new(FakeClock::new(1000));
    let approval = Arc::new(Approval);
    let mut core = KeelessCore::new(host(config.clone(), approval, clock))
        .await
        .unwrap();
    config.fail_save.store(true, Ordering::Relaxed);

    assert!(matches!(
        operations::set_config::run(
            &mut core,
            KeelessConfigPatch {
                auto_lock_timeout_ms: Some(Some(25)),
                paranoia_mode: Some(true),
            }
        )
        .await,
        Err(CoreError::Host(_))
    ));
    assert_eq!(core.settings, KeelessConfig::default());
}

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
    operations::reveal_entry_field::run(&mut core, entry_id.clone(), password_id.clone(), None)
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

    operations::reveal_entry_field::run(&mut core, entry_id.clone(), password_id, None)
        .await
        .unwrap();
    let _ = core.sync(None).await;
    operations::update_entry::run(&mut core, entry_id, fields, None, None)
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
        operations::update_tag_style::run(
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

    operations::delete_tag::run(
        &mut core,
        DeleteTagArgs {
            name: " orphan ".into(),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        operations::delete_tag::run(
            &mut core,
            DeleteTagArgs {
                name: "shared".into()
            }
        )
        .await,
        Err(CoreError::TagInUse)
    ));
    assert!(matches!(
        operations::update_tag_style::run(
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
        operations::update_tag_style::run(
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
        operations::update_tag_style::run(
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
        operations::add_entry_from_template::run(
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
        operations::add_entry_from_template::run(
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
                .filter(|(_, field)| template::is_internal_field(field.name()))
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
        operations::add_entry_from_template::run(
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

    operations::move_entry::run(
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

    operations::move_entry::run(
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
        operations::move_entry::run(
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
        operations::move_entry::run(
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

    operations::move_group::run(
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
        operations::move_group::run(
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
        operations::move_group::run(
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
        operations::delete_group::run(&mut core, DeleteGroupArgs { group_id: missing },).await,
        Err(CoreError::GroupNotFound)
    ));
    assert!(matches!(
        operations::delete_group::run(
            &mut core,
            DeleteGroupArgs {
                group_id: schema_id(ids.root_group),
            },
        )
        .await,
        Err(CoreError::InvalidGroupDelete)
    ));

    operations::delete_group::run(
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
        operations::delete_group::run(
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
            EntryFieldInformation::Field { .. } => unreachable!(),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ordered_types,
        ["divider", "confirmation", "override", "expiry", "tags"]
    );
    assert!(detail.fields.iter().all(|field| match field {
        EntryFieldInformation::Field { order, .. }
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
    operations::update_entry::run(
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
            operations::reveal_entry_field::run(&mut core, entry_id.clone(), field_id, None)
                .await
                .unwrap()
                .value,
            expected
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
        operations::update_entry::run(
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
        operations::update_entry::run(
            &mut core,
            schema_id(ids.root_entry),
            current_fields.clone(),
            None,
            None,
        )
        .await,
        Err(CoreError::PasswordRequired)
    ));
    operations::update_entry::run(
        &mut core,
        schema_id(ids.root_entry),
        current_fields,
        None,
        Some(b"correct"),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn delete_entry_requires_trash_for_permanent_removal() {
    let (mut core, ids) = query_core().await;
    let entry_id = schema_id(ids.nested_entry);
    assert!(matches!(
        operations::delete_entry::run(
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

    operations::delete_entry::run(
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
        operations::delete_entry::run(
            &mut core,
            DeleteEntryArgs {
                entry_id: entry_id.clone(),
                permanent: false,
            }
        )
        .await,
        Err(CoreError::InvalidEntryDelete)
    ));
    operations::delete_entry::run(
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
        operations::add_entry::run(
            &mut core,
            AddEntryArgs {
                parent_group_id: missing.clone(),
            },
        )
        .await,
        Err(CoreError::GroupNotFound)
    ));
    assert!(matches!(
        operations::add_group::run(
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

    let entry = operations::add_entry::run(
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

    let group = operations::add_group::run(
        &mut core,
        AddGroupArgs {
            parent_group_id: schema_id(ids.child_group),
        },
    )
    .await
    .unwrap();
    operations::rename_group::run(
        &mut core,
        RenameGroupArgs {
            group_id: group.id.clone(),
            name: "  Renamed Group  ".into(),
        },
    )
    .await
    .unwrap();
    operations::update_group::run(
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
        operations::rename_group::run(
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

#[tokio::test]
async fn reveal_entry_field_handles_ids_duplicates_and_credentials() {
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
        operations::reveal_entry_field::run(&mut core, entry_id.clone(), password_id.clone(), None)
            .await
            .unwrap()
            .value,
        "password-secret"
    );
    assert_eq!(
        operations::reveal_entry_field::run(&mut core, entry_id.clone(), duplicate_first_id, None)
            .await
            .unwrap()
            .value,
        "duplicate-first"
    );
    assert_eq!(
        operations::reveal_entry_field::run(&mut core, entry_id.clone(), duplicate_second_id, None)
            .await
            .unwrap()
            .value,
        "duplicate-second"
    );
    assert!(matches!(
        operations::reveal_entry_field::run(&mut core, entry_id.clone(), public_id, None).await,
        Err(CoreError::InvalidEntryField)
    ));
    assert!(matches!(
        operations::reveal_entry_field::run(&mut core, entry_id.clone(), "invalid".into(), None)
            .await,
        Err(CoreError::InvalidEntryField)
    ));
    assert_eq!(
        keeless_schema::OperationError::from(&CoreError::InvalidEntryField).code,
        "invalid_entry_field"
    );
    assert!(matches!(
        operations::reveal_entry_field::run(
            &mut core,
            schema_id(Uuid::from_u128(999)),
            password_id.clone(),
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
        operations::reveal_entry_field::run(&mut core, entry_id.clone(), password_id.clone(), None)
            .await,
        Err(CoreError::PasswordRequired)
    ));
    assert!(matches!(
        operations::reveal_entry_field::run(
            &mut core,
            entry_id.clone(),
            password_id.clone(),
            Some(b"wrong"),
        )
        .await,
        Err(CoreError::InvalidCredentials)
    ));
    assert_eq!(
        operations::reveal_entry_field::run(
            &mut core,
            entry_id.clone(),
            notes_id,
            Some(b"correct")
        )
        .await
        .unwrap()
        .value,
        "notes-secret"
    );

    operations::lock::run(&mut core);
    assert!(matches!(
        operations::reveal_entry_field::run(
            &mut core,
            DatabaseNodeId::Uuid("invalid".into()),
            password_id,
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

#[tokio::test]
async fn mutation_journal_is_encrypted_atomic_replayable_and_sequenced() {
    let (bytes, _) = query_database_bytes(b"correct");
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(bytes.clone()))));
    let persistence = Arc::new(MemoryDatabasePersistence::default());
    let (mut core, ids) = query_core_with_persistence(storage.clone(), persistence.clone()).await;

    let first = operations::add_entry::run(
        &mut core,
        AddEntryArgs {
            parent_group_id: schema_id(ids.child_group),
        },
    )
    .await
    .unwrap();
    let first_id = model_id(first.id.clone());
    let second = operations::add_entry::run(
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
        assert_eq!(envelope["version"], 1);
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
        operations::add_entry::run(
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
            .encode_cache(&cached_database)
            .unwrap(),
    );
    *storage.0.lock().unwrap() = None;
    let (mut replayed, _) = query_core_with_persistence(storage, persistence).await;
    let database = replayed.handle.as_ref().unwrap().database();
    assert!(database.get_entry(&first_id).is_some());
    assert!(database.get_entry(&model_id(second.id)).is_some());
    assert!(!replayed.handle.as_ref().unwrap().is_dirty());
    assert_eq!(replayed.sync_status, SyncStatus::Syncing);
    replayed.tick().await;
    assert_eq!(replayed.sync_status, SyncStatus::Error);
    assert!(replayed.sync_error.is_some());
    assert!(replayed.handle.is_some());
}

#[test]
fn credential_vault_wraps_and_debug_redacts() {
    let raw = SecureArray::from_slice(&[42; 32]).unwrap();
    let vault = CredentialVault::wrap(&raw).unwrap();
    assert!(vault.raw_key_matches(&[42; 32]).unwrap());
    assert_eq!(
        format!("{vault:?}"),
        "CredentialVault { credential: \"[REDACTED]\" }"
    );
}
