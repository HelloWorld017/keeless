use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
};

use keeless_kdbx::kdbx::template;
use keeless_kdbx::{
    CompositeCredentials, Database, DatabaseVersion, Entry, EntryBinary, EntryFieldSelector, Group,
    IconImageCustom, IconImageStandard, NodeId, ProtectedString, save_database,
};
use keeless_schema::{
    AddEntryArgs, AddEntryFromTemplateArgs, AddGroupArgs, DatabaseNodeId, DeleteEntryArgs,
    DeleteGroupArgs, DeleteTagArgs, EmptyRecycleBinArgs, EntryFieldInformation,
    EntryFieldUpdate as SchemaEntryFieldUpdate, EntryPropertiesUpdate, FieldControl,
    GetEntriesArgs, GetEntryDetailArgs, GetGroupEntriesArgs, GetTagEntriesArgs, IconReference,
    MoveEntryArgs, MoveGroupArgs, OpenTarget, Operation, OperationSuccess, RenameGroupArgs,
    SearchEntriesArgs, SearchFuzzyArgs, TagStyle, UpdateGroupArgs, UpdateTagStyleArgs,
};
use keeless_sync::{
    ByteRange, FileMetadata, RemoteFile, StorageError, StorageErrorKind, StorageFuture,
    WriteCondition, WriteOutcome,
};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::*;

mod passkeys;

pub(super) struct DetailField<'a> {
    pub(super) order: u64,
    pub(super) field_id: Option<&'a str>,
    pub(super) kind: &'a keeless_schema::EntryFieldKind,
    pub(super) name: &'a str,
    pub(super) label: &'a str,
    pub(super) value: Option<&'a str>,
    pub(super) is_protected: bool,
    pub(super) is_internal: bool,
    pub(super) control: Option<&'a FieldControl>,
}

pub(super) fn detail_field(field: &EntryFieldInformation) -> Option<DetailField<'_>> {
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
pub(super) struct MemoryConfig {
    pub(super) value: Mutex<Option<Vec<u8>>>,
    pub(super) fail_save: AtomicBool,
}

impl keeless_lesswire::StateStore for MemoryConfig {
    fn load(&self) -> keeless_lesswire::WireFuture<'_, keeless_lesswire::Result<Option<Vec<u8>>>> {
        Box::pin(async { Ok(self.value.lock().unwrap().clone()) })
    }

    fn save<'a>(
        &'a self,
        value: &'a [u8],
    ) -> keeless_lesswire::WireFuture<'a, keeless_lesswire::Result<()>> {
        Box::pin(async move {
            if self.fail_save.load(Ordering::Relaxed) {
                return Err(keeless_lesswire::Error::Host("save failed".into()));
            }
            *self.value.lock().unwrap() = Some(value.to_vec());
            Ok(())
        })
    }
}

#[derive(Default)]
pub(super) struct MemoryDatabasePersistence {
    pub(super) cache: Mutex<Option<Vec<u8>>>,
    pub(super) journal: Mutex<Vec<Vec<u8>>>,
    pub(super) selected: Mutex<Option<DatabaseId>>,
    pub(super) fail_append: AtomicBool,
    pub(super) fail_state_write: AtomicBool,
    pub(super) state: Mutex<std::collections::HashMap<String, Vec<u8>>>,
}

impl DatabasePersistence for MemoryDatabasePersistence {
    fn select<'a>(&'a self, database_id: &'a DatabaseId) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            *self.selected.lock().unwrap() = Some(database_id.clone());
            Ok(())
        })
    }

    fn purge<'a>(&'a self, _database_id: &'a DatabaseId) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            *self.cache.lock().unwrap() = None;
            self.journal.lock().unwrap().clear();
            self.state.lock().unwrap().clear();
            Ok(())
        })
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

    fn read_state_record<'a>(&'a self, name: &'a str) -> HostFuture<'a, Result<Option<Vec<u8>>>> {
        Box::pin(async move { Ok(self.state.lock().unwrap().get(name).cloned()) })
    }

    fn write_state_record<'a>(
        &'a self,
        name: &'a str,
        bytes: &'a [u8],
    ) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            if self.fail_state_write.load(Ordering::Relaxed) {
                return Err(CoreError::Host("state write failed".into()));
            }
            self.state
                .lock()
                .unwrap()
                .insert(name.into(), bytes.to_vec());
            Ok(())
        })
    }
}

pub(super) struct Approval;

impl ConnectionApprovalProvider for Approval {
    fn approve_connection(&self, _: ConnectionApprovalRequest) -> HostFuture<'_, Result<bool>> {
        Box::pin(async { Ok(true) })
    }
}

pub(super) struct PasswordInput {
    pub(super) password: Option<Vec<u8>>,
    pub(super) modes: Mutex<Vec<PasswordInputMode>>,
}

pub(super) struct PasskeyConsent {
    pub(super) selected: Option<usize>,
    pub(super) requests: Mutex<Vec<PasskeyConsentRequest>>,
}

impl PasskeyConsent {
    pub(super) fn approve(selected: usize) -> Self {
        Self {
            selected: Some(selected),
            requests: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn cancelled() -> Self {
        Self {
            selected: None,
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl PasskeyConsentProvider for PasskeyConsent {
    fn request_passkey_consent(
        &self,
        request: PasskeyConsentRequest,
    ) -> HostFuture<'_, Result<Option<usize>>> {
        self.requests.lock().unwrap().push(request);
        Box::pin(async { Ok(self.selected) })
    }
}

impl PasswordInput {
    pub(super) fn new(password: &[u8]) -> Self {
        Self {
            password: Some(password.to_vec()),
            modes: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn cancelled() -> Self {
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

pub(super) struct FakeClock {
    pub(super) wall: AtomicI64,
    pub(super) monotonic: AtomicU64,
}

impl FakeClock {
    pub(super) fn new(millis: u64) -> Self {
        Self {
            wall: AtomicI64::new(millis as i64),
            monotonic: AtomicU64::new(millis),
        }
    }

    pub(super) fn set(&self, millis: u64) {
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

impl keeless_lesswire::Clock for FakeClock {
    fn now_millis(&self) -> i64 {
        self.wall.load(Ordering::Relaxed)
    }

    fn monotonic_millis(&self) -> u64 {
        self.monotonic.load(Ordering::Relaxed)
    }
}

pub(super) struct MemoryStorage(pub(super) Mutex<Option<Vec<u8>>>);

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

pub(super) fn persistent_storage(provider: Arc<dyn StorageProvider>) -> Arc<Storage> {
    Arc::new(Storage::persistent(provider))
}

#[derive(Default)]
pub(super) struct MemoryTransferProvider {
    uploads: Mutex<std::collections::HashMap<String, Zeroizing<Vec<u8>>>>,
    downloads: Mutex<std::collections::HashMap<String, Vec<u8>>>,
    next_id: AtomicU64,
}

impl MemoryTransferProvider {
    pub(super) fn add_upload(&self, transfer_id: &str, bytes: Vec<u8>) {
        self.uploads
            .lock()
            .unwrap()
            .insert(transfer_id.into(), Zeroizing::new(bytes));
    }

    pub(super) fn download(&self, transfer_id: &str) -> Option<Vec<u8>> {
        self.downloads.lock().unwrap().get(transfer_id).cloned()
    }
}

impl TransferProvider for MemoryTransferProvider {
    fn publish_download(&self, _owner: &str, bytes: Zeroizing<Vec<u8>>) -> Result<String> {
        let transfer_id = format!("download-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        self.downloads
            .lock()
            .unwrap()
            .insert(transfer_id.clone(), bytes.to_vec());
        Ok(transfer_id)
    }

    fn consume_upload(&self, _owner: &str, transfer_id: &str) -> Result<Zeroizing<Vec<u8>>> {
        self.uploads
            .lock()
            .unwrap()
            .remove(transfer_id)
            .ok_or_else(|| CoreError::Host("upload transfer does not exist".into()))
    }

    fn clear(&self) {
        self.uploads.lock().unwrap().clear();
        self.downloads.lock().unwrap().clear();
    }
}

pub(super) fn metadata(bytes: &[u8]) -> FileMetadata {
    FileMetadata {
        size: bytes.len() as u64,
        revision: None,
        last_modified: None,
    }
}

pub(super) fn database_bytes(password: &[u8]) -> Vec<u8> {
    let mut database = Database::new(DatabaseVersion::KDBX4);
    let root_id = NodeId::new_uuid();
    let mut root = Group::new(root_id);
    root.title = "Root".into();
    database.groups.insert(root_id, root);
    database.root_group_id = Some(root_id);
    let credentials = CompositeCredentials::new().with_password(password).unwrap();
    let key = keeless_kdbx::initialize_database_key(&mut database, &credentials).unwrap();
    let mut bytes = Vec::new();
    save_database(&mut bytes, &database, &key).unwrap();
    bytes
}

#[derive(Clone, Copy)]
pub(super) struct QueryIds {
    pub(super) root_group: Uuid,
    pub(super) child_group: Uuid,
    pub(super) nested_group: Uuid,
    pub(super) root_entry: Uuid,
    pub(super) child_entry: Uuid,
    pub(super) nested_entry: Uuid,
}

pub(super) fn query_database_bytes(password: &[u8]) -> (Vec<u8>, QueryIds) {
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

    let credentials = CompositeCredentials::new().with_password(password).unwrap();
    let key = keeless_kdbx::initialize_database_key(&mut database, &credentials).unwrap();
    let mut bytes = Vec::new();
    save_database(&mut bytes, &database, &key).unwrap();
    (bytes, ids)
}

pub(super) async fn query_core() -> (KeelessCore, QueryIds) {
    let (bytes, ids) = query_database_bytes(b"correct");
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(bytes))));
    let mut providers: HashMap<String, Arc<Storage>> = HashMap::new();
    providers.insert("memory".into(), persistent_storage(storage));
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
        OpenTarget::Storage {
            storage: StorageDescriptor {
                provider: "memory".into(),
                path: "vault.kdbx".into(),
            },
        },
    )
    .await
    .unwrap();
    operations::unlock::run(&mut core, b"correct")
        .await
        .unwrap();
    (core, ids)
}

pub(super) async fn query_core_with_persistence(
    storage: Arc<MemoryStorage>,
    persistence: Arc<MemoryDatabasePersistence>,
) -> (KeelessCore, QueryIds) {
    let (_, ids) = query_database_bytes(b"correct");
    let mut providers: HashMap<String, Arc<Storage>> = HashMap::new();
    providers.insert("memory".into(), persistent_storage(storage));
    let mut core = KeelessCore::new(KeelessHost {
        storage_providers: providers,
        untrusted_state: Arc::new(MemoryConfig::default()),
        core_state: Arc::new(MemoryConfig::default()),
        connection_approval: Arc::new(Approval),
        password_input: None,
        passkey_consent: Some(Arc::new(PasskeyConsent::approve(0))),
        clock: Arc::new(FakeClock::new(1234)),
        database_persistence: persistence,
        task_spawner: None,
        transfer_provider: None,
    })
    .await
    .unwrap();
    operations::open::run(
        &mut core,
        OpenTarget::Storage {
            storage: StorageDescriptor {
                provider: "memory".into(),
                path: "vault.kdbx".into(),
            },
        },
    )
    .await
    .unwrap();
    operations::unlock::run(&mut core, b"correct")
        .await
        .unwrap();
    (core, ids)
}

pub(super) fn schema_id(id: Uuid) -> DatabaseNodeId {
    DatabaseNodeId::Uuid(id.hyphenated().to_string())
}

pub(super) fn model_id(id: DatabaseNodeId) -> NodeId {
    match id {
        DatabaseNodeId::Uuid(value) => NodeId::from_uuid(Uuid::parse_str(&value).unwrap()),
        DatabaseNodeId::Int(value) => NodeId::from_int(value),
    }
}

pub(super) fn host(
    config: Arc<dyn keeless_lesswire::StateStore>,
    _approval: Arc<Approval>,
    clock: Arc<FakeClock>,
) -> KeelessHost {
    KeelessHost {
        storage_providers: HashMap::new(),
        untrusted_state: config,
        core_state: Arc::new(MemoryConfig::default()),
        connection_approval: Arc::new(Approval),
        password_input: None,
        passkey_consent: Some(Arc::new(PasskeyConsent::approve(0))),
        clock,
        database_persistence: Arc::new(MemoryDatabasePersistence::default()),
        task_spawner: None,
        transfer_provider: None,
    }
}

mod config;
mod credential;
mod credential_vault;
mod entries;
mod lifecycle;
mod mutations;
mod network;
mod persistence;
mod queries;
