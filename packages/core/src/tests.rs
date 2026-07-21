use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::SigningKey;
use keeless_kdbx::{
    Database, DatabaseVersion, Entry, EntryBinary, EntryFieldSelector, Group, IconImageCustom,
    IconImageStandard, NodeId, ProtectedString, save_database,
};
use keeless_schema::{
    AddEntryArgs, AddEntryFromTemplateArgs, AddGroupArgs, DatabaseNodeId, DatabaseStatusResult,
    DeleteEntryArgs, DeleteGroupArgs, EntryFieldUpdate as SchemaEntryFieldUpdate,
    GetDatabaseStatusArgs, GetEntriesArgs, GetEntryDetailArgs, GetGroupEntriesArgs,
    GetTagEntriesArgs, MoveEntryArgs, MoveGroupArgs, Operation, OperationOutcome, OperationRequest,
    OperationResponse, OperationSuccess, RenameGroupArgs,
};
use keeless_sync::{
    ByteRange, FileMetadata, RemoteFile, StorageError, StorageErrorKind, StorageFuture,
    WriteCondition, WriteOutcome,
};
use uuid::Uuid;
use x25519_dalek::{PublicKey, StaticSecret};

use super::*;
use crate::protocol::{decrypt_frame, encrypt_frame, handshake_frame, public_key_bundle};

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

struct Approval(AtomicBool);

impl ClientApprovalProvider for Approval {
    fn approve(&self, _: &str) -> HostFuture<'_, Result<bool>> {
        Box::pin(async { Ok(self.0.load(Ordering::Relaxed)) })
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
            Arc::new(Approval(AtomicBool::new(true))),
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
    approval: Arc<Approval>,
    clock: Arc<FakeClock>,
) -> KeelessHost {
    KeelessHost {
        default_approved_keys: Vec::new(),
        storage_providers: HashMap::new(),
        config_provider: config,
        approval_provider: approval,
        clock,
    }
}

fn client_identity() -> (SigningKey, StaticSecret, String) {
    let signing = SigningKey::from_bytes(&[7; 32]);
    let encryption = StaticSecret::from([9; 32]);
    let bundle = public_key_bundle(&signing, &PublicKey::from(&encryption));
    (signing, encryption, bundle)
}

#[test]
fn public_key_bundles_reject_weak_signing_keys() {
    let weak_bundle = format!(
        "v1.{}.{}",
        URL_SAFE_NO_PAD.encode([0; 32]),
        URL_SAFE_NO_PAD.encode([9; 32])
    );
    assert!(parse_public_key_bundle(&weak_bundle).is_none());
}

#[tokio::test]
async fn generates_and_reloads_identity_without_exposing_secrets_in_debug() {
    let config = Arc::new(MemoryConfig::default());
    let approval = Arc::new(Approval(AtomicBool::new(true)));
    let clock = Arc::new(FakeClock::new(1000));
    let core = KeelessCore::new(host(config.clone(), approval.clone(), clock.clone()))
        .await
        .unwrap();
    let bundle = core.public_key_bundle().unwrap();
    let debug = format!("{core:?}");
    assert!(debug.contains("REDACTED"));
    assert!(!debug.contains(&bundle));

    let reloaded = KeelessCore::new(host(config, approval, clock))
        .await
        .unwrap();
    assert_eq!(reloaded.public_key_bundle().unwrap(), bundle);
}

#[tokio::test]
async fn config_patch_is_deep_and_paranoia_is_persisted() {
    let config = Arc::new(MemoryConfig::default());
    let mut core = KeelessCore::new(host(
        config.clone(),
        Arc::new(Approval(AtomicBool::new(true))),
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
async fn failed_persistence_rolls_back_settings_and_client_approval() {
    let config = Arc::new(FailingConfig::default());
    let clock = Arc::new(FakeClock::new(1000));
    let approval = Arc::new(Approval(AtomicBool::new(true)));
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

    let (signing, _, bundle) = client_identity();
    let frame = handshake_frame(1000, bundle.clone(), &signing).unwrap();
    assert!(matches!(
        core.handle_frame(&frame).await,
        Err(CoreError::Host(_))
    ));
    assert!(!core.approved_clients.contains(&bundle));
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
        ..host(
            config,
            Arc::new(Approval(AtomicBool::new(true))),
            clock.clone(),
        )
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
async fn create_builds_and_unlocks_a_new_database_without_overwriting() {
    let storage = Arc::new(MemoryStorage(Mutex::new(None)));
    let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
    providers.insert("memory".into(), storage.clone());
    let mut core = KeelessCore::new(KeelessHost {
        storage_providers: providers,
        ..host(
            Arc::new(MemoryConfig::default()),
            Arc::new(Approval(AtomicBool::new(true))),
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
async fn handshake_approves_persists_and_replay_is_dropped() {
    let config = Arc::new(MemoryConfig::default());
    let clock = Arc::new(FakeClock::new(10_000));
    let mut core = KeelessCore::new(host(
        config.clone(),
        Arc::new(Approval(AtomicBool::new(true))),
        clock,
    ))
    .await
    .unwrap();
    let (signing, _, bundle) = client_identity();
    let frame = handshake_frame(10_000, bundle.clone(), &signing).unwrap();
    let response = core.handle_frame(&frame).await.unwrap().unwrap();
    assert!(response.payload.is_none());
    assert!(response.ephemeral_public_key.is_none());
    assert!(core.handle_frame(&frame).await.unwrap().is_none());
    let saved: serde_json::Value =
        serde_json::from_slice(&config.0.lock().unwrap().clone().unwrap()).unwrap();
    assert!(
        saved["approvedClientBundles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == &bundle)
    );
}

#[tokio::test]
async fn denied_handshakes_consume_the_bounded_nonce_pool() {
    let mut core = KeelessCore::new(host(
        Arc::new(MemoryConfig::default()),
        Arc::new(Approval(AtomicBool::new(false))),
        Arc::new(FakeClock::new(10_000)),
    ))
    .await
    .unwrap();
    let (signing, _, bundle) = client_identity();
    let frame = handshake_frame(10_000, bundle, &signing).unwrap();

    assert!(core.handle_frame(&frame).await.unwrap().is_none());
    assert_eq!(core.nonce_cache.len(), 1);
    assert!(core.handle_frame(&frame).await.unwrap().is_none());
    assert_eq!(core.nonce_cache.len(), 1);
}

#[tokio::test]
async fn failed_reunlock_preserves_an_existing_unlocked_handle() {
    let clock = Arc::new(FakeClock::new(100));
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(database_bytes(b"correct")))));
    let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
    providers.insert("memory".into(), storage.clone());
    let mut core = KeelessCore::new(KeelessHost {
        storage_providers: providers,
        ..host(
            Arc::new(MemoryConfig::default()),
            Arc::new(Approval(AtomicBool::new(true))),
            clock,
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
async fn handshake_rejects_non_contributory_client_encryption_key() {
    let clock = Arc::new(FakeClock::new(10_000));
    let mut core = KeelessCore::new(host(
        Arc::new(MemoryConfig::default()),
        Arc::new(Approval(AtomicBool::new(true))),
        clock,
    ))
    .await
    .unwrap();
    let signing = SigningKey::from_bytes(&[7; 32]);
    let mut low_order = [0; 32];
    low_order[0] = 1;
    let bundle = format!(
        "v1.{}.{}",
        URL_SAFE_NO_PAD.encode(signing.verifying_key().as_bytes()),
        URL_SAFE_NO_PAD.encode(low_order)
    );
    let frame = handshake_frame(10_000, bundle, &signing).unwrap();

    assert!(core.handle_frame(&frame).await.unwrap().is_none());
}

#[tokio::test]
async fn encrypted_status_round_trip_and_tampering_drop() {
    let config = Arc::new(MemoryConfig::default());
    let clock = Arc::new(FakeClock::new(5000));
    let (client_signing, client_secret, client_bundle) = client_identity();
    let mut core = KeelessCore::new(KeelessHost {
        default_approved_keys: vec![client_bundle.clone()],
        ..host(config, Arc::new(Approval(AtomicBool::new(false))), clock)
    })
    .await
    .unwrap();
    let request = OperationRequest {
        request_id: "request-1".into(),
        operation: Operation::GetDatabaseStatus(GetDatabaseStatusArgs {}),
    };
    let core_bundle = parse_public_key_bundle(&core.public_key_bundle().unwrap()).unwrap();
    let frame = encrypt_frame(
        5000,
        &serde_json::to_vec(&request).unwrap(),
        client_bundle,
        &client_signing,
        &core_bundle.encryption,
    )
    .unwrap();
    let response = core.handle_frame(&frame).await.unwrap().unwrap();
    let plaintext = decrypt_frame(&response, &client_secret).unwrap();
    let response: OperationResponse = serde_json::from_slice(&plaintext).unwrap();
    assert_eq!(response.request_id, "request-1");
    assert!(matches!(
        response.outcome,
        OperationOutcome::Success {
            success: OperationSuccess::GetDatabaseStatus(DatabaseStatusResult {
                status: DatabaseStatus::NotExist
            })
        }
    ));

    let mut tampered = frame;
    tampered.nonce = URL_SAFE_NO_PAD.encode([1; 24]);
    assert!(core.handle_frame(&tampered).await.unwrap().is_none());
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
        assert!(database.add_entry(direct, &templates_id));

        let mut nested = Entry::new(nested_template_id);
        nested.set_title("Nested Template");
        nested.tags = vec!["template-only".into()];
        assert!(database.add_entry(nested, &nested_group_id));
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
        template.is_template = true;
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
        ),
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
        assert!(!copied.is_template);
    }

    core.credential = None;
    let redacted_id = model_id(
        operations::add_entry_from_template::run(
            &mut core,
            AddEntryFromTemplateArgs {
                parent_group_id: schema_id(ids.child_group),
                template_entry_id: schema_id(Uuid::from_u128(41)),
            },
        )
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
        ),
        Err(CoreError::InvalidEntryMove)
    ));
    assert!(matches!(
        operations::move_entry::run(
            &mut core,
            MoveEntryArgs {
                entry_id: schema_id(Uuid::from_u128(998)),
                parent_group_id: schema_id(ids.root_group),
            },
        ),
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
        ),
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
        ),
        Err(CoreError::InvalidGroupMove)
    ));
}

#[tokio::test]
async fn delete_group_operation_moves_the_subtree_to_trash_and_protects_special_groups() {
    let (mut core, ids) = query_core().await;
    let missing = schema_id(Uuid::from_u128(999));

    assert!(matches!(
        operations::delete_group::run(&mut core, DeleteGroupArgs { group_id: missing },),
        Err(CoreError::GroupNotFound)
    ));
    assert!(matches!(
        operations::delete_group::run(
            &mut core,
            DeleteGroupArgs {
                group_id: schema_id(ids.root_group),
            },
        ),
        Err(CoreError::InvalidGroupDelete)
    ));

    operations::delete_group::run(
        &mut core,
        DeleteGroupArgs {
            group_id: schema_id(ids.child_group),
        },
    )
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
        ),
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
            .find(|field| field.name == name)
            .unwrap()
    };
    assert_eq!(field("Title").value.as_deref(), Some("Root Entry"));
    assert_eq!(field("Title").field_id, "standard:Title");
    assert_eq!(field("Title").kind, keeless_schema::EntryFieldKind::Title);
    assert_eq!(field("UserName").field_id, "standard:UserName");
    assert_eq!(field("Password").field_id, "standard:Password");
    assert_eq!(field("URL").field_id, "standard:URL");
    assert_eq!(field("Notes").field_id, "standard:Notes");
    assert_eq!(field("Public").kind, keeless_schema::EntryFieldKind::Custom);
    assert!(Uuid::parse_str(&field("Public").field_id).is_ok());
    assert_eq!(field("UserName").value.as_deref(), Some("alice"));
    assert_eq!(field("URL").value.as_deref(), Some("https://example.test"));
    assert_eq!(field("Public").value.as_deref(), Some("public-value"));
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
async fn update_entry_applies_one_atomic_history_change_and_preserves_duplicate_secrets() {
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
            .filter(|field| field.name == name)
            .nth(occurrence)
            .unwrap()
            .field_id
            .clone()
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
    ];
    operations::update_entry::run(&mut core, entry_id.clone(), fields, None).unwrap();

    let entry = core
        .handle
        .as_ref()
        .unwrap()
        .database()
        .get_entry(&model_id(entry_id.clone()))
        .unwrap();
    assert_eq!(entry.history.len(), old.history.len() + 1);
    assert_eq!(
        entry.history.last().unwrap().last_modification_time,
        old.last_modification_time
    );
    assert_eq!(
        entry
            .custom_fields()
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
            .find(|field| field.name == name)
            .unwrap()
            .field_id
            .clone();
        assert_eq!(
            operations::reveal_entry_field::run(&mut core, entry_id.clone(), field_id, None)
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
            None,
        ),
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
    .map(|field| SchemaEntryFieldUpdate {
        field_id: Some(field.field_id),
        name: field.name,
        value: field.value,
        is_protected: field.is_protected,
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
        ),
        Err(CoreError::PasswordRequired)
    ));
    operations::update_entry::run(
        &mut core,
        schema_id(ids.root_entry),
        current_fields,
        Some(b"correct"),
    )
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
        ),
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
        ),
        Err(CoreError::InvalidEntryDelete)
    ));
    operations::delete_entry::run(
        &mut core,
        DeleteEntryArgs {
            entry_id: entry_id.clone(),
            permanent: true,
        },
    )
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
        ),
        Err(CoreError::GroupNotFound)
    ));
    assert!(matches!(
        operations::add_group::run(
            &mut core,
            AddGroupArgs {
                parent_group_id: missing,
            },
        ),
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
    .unwrap();
    let detail =
        operations::get_entry_detail::run(&mut core, GetEntryDetailArgs { entry_id: entry.id })
            .unwrap();
    assert_eq!(detail.fields[0].value.as_deref(), Some(""));

    let group = operations::add_group::run(
        &mut core,
        AddGroupArgs {
            parent_group_id: schema_id(ids.child_group),
        },
    )
    .unwrap();
    operations::rename_group::run(
        &mut core,
        RenameGroupArgs {
            group_id: group.id.clone(),
            name: "  Renamed Group  ".into(),
        },
    )
    .unwrap();
    let hierarchy = operations::get_group_hierarchy::run(&mut core).unwrap();
    let created_group = hierarchy
        .groups
        .iter()
        .find(|candidate| candidate.id == group.id)
        .unwrap();
    assert_eq!(created_group.name, "Renamed Group");
    assert_eq!(created_group.icon.standard_id, 48);
    assert!(matches!(
        operations::rename_group::run(
            &mut core,
            RenameGroupArgs {
                group_id: schema_id(ids.child_group),
                name: "  ".into(),
            },
        ),
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
            .filter(|field| field.name == name)
            .nth(occurrence)
            .unwrap()
            .field_id
            .clone()
    };
    let password_id = field_id("Password", 0);
    let notes_id = field_id("Notes", 0);
    let public_id = field_id("Public", 0);
    let duplicate_first_id = field_id("Duplicate", 0);
    let duplicate_second_id = field_id("Duplicate", 1);

    assert_eq!(
        operations::reveal_entry_field::run(&mut core, entry_id.clone(), password_id.clone(), None)
            .unwrap()
            .value,
        "password-secret"
    );
    assert_eq!(
        operations::reveal_entry_field::run(&mut core, entry_id.clone(), duplicate_first_id, None)
            .unwrap()
            .value,
        "duplicate-first"
    );
    assert_eq!(
        operations::reveal_entry_field::run(&mut core, entry_id.clone(), duplicate_second_id, None)
            .unwrap()
            .value,
        "duplicate-second"
    );
    assert!(matches!(
        operations::reveal_entry_field::run(&mut core, entry_id.clone(), public_id, None),
        Err(CoreError::InvalidEntryField)
    ));
    assert!(matches!(
        operations::reveal_entry_field::run(&mut core, entry_id.clone(), "invalid".into(), None),
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
        ),
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
        operations::reveal_entry_field::run(&mut core, entry_id.clone(), password_id.clone(), None),
        Err(CoreError::PasswordRequired)
    ));
    assert!(matches!(
        operations::reveal_entry_field::run(
            &mut core,
            entry_id.clone(),
            password_id.clone(),
            Some(b"wrong"),
        ),
        Err(CoreError::InvalidCredentials)
    ));
    assert_eq!(
        operations::reveal_entry_field::run(
            &mut core,
            entry_id.clone(),
            notes_id,
            Some(b"correct")
        )
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
        ),
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
async fn timestamp_boundaries_and_nonce_expiry_are_inclusive() {
    let config = Arc::new(MemoryConfig::default());
    let clock = Arc::new(FakeClock::new(1000));
    let mut core = KeelessCore::new(host(
        config,
        Arc::new(Approval(AtomicBool::new(true))),
        clock.clone(),
    ))
    .await
    .unwrap();
    let (signing, _, bundle) = client_identity();
    let boundary = handshake_frame(500, bundle.clone(), &signing).unwrap();
    assert!(core.handle_frame(&boundary).await.unwrap().is_some());
    let stale = handshake_frame(499, bundle.clone(), &signing).unwrap();
    assert!(core.handle_frame(&stale).await.unwrap().is_none());

    clock.set(1501);
    let fresh = handshake_frame(1501, bundle, &signing).unwrap();
    assert!(core.handle_frame(&fresh).await.unwrap().is_some());
    assert_eq!(core.nonce_cache.len(), 1);
}

#[tokio::test]
async fn full_nonce_cache_rejects_without_eviction() {
    let clock = Arc::new(FakeClock::new(2000));
    let mut core = KeelessCore::new(host(
        Arc::new(MemoryConfig::default()),
        Arc::new(Approval(AtomicBool::new(true))),
        clock,
    ))
    .await
    .unwrap();
    for index in 0..NONCE_CACHE_CAPACITY {
        core.nonce_cache.insert(format!("nonce-{index}"), 2000);
    }
    let (signing, _, bundle) = client_identity();
    let frame = handshake_frame(2000, bundle, &signing).unwrap();
    assert!(core.handle_frame(&frame).await.unwrap().is_none());
    assert_eq!(core.nonce_cache.len(), NONCE_CACHE_CAPACITY);
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
