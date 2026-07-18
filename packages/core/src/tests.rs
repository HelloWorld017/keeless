use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::SigningKey;
use keeless_kdbx::{
    Database, DatabaseVersion, Entry, EntryBinary, EntryField, Group, IconImageCustom,
    IconImageStandard, NodeId, ProtectedString, save_database,
};
use keeless_schema::{
    DatabaseNodeId, DatabaseStatusResult, GetDatabaseStatusArgs, GetEntryDetailArgs,
    GetGroupEntriesArgs, Operation, OperationOutcome, OperationRequest, OperationResponse,
    OperationSuccess,
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
    root_entry.title = "Root Entry".into();
    root_entry.username = ProtectedString::new_plain("alice");
    root_entry.password = ProtectedString::new_protected("password-secret");
    root_entry.url = "https://example.test".into();
    root_entry.notes = ProtectedString::new_protected("notes-secret");
    root_entry.icon = keeless_kdbx::IconImage::Standard(IconImageStandard::new(7));
    root_entry.tags = vec!["shared".into(), "work".into(), "shared".into()];
    root_entry.background_color = "#000000".into();
    root_entry.foreground_color = "#ffffff".into();
    root_entry.override_url = "cmd://open".into();
    root_entry.usage_count = 3;
    root_entry.custom_fields = vec![
        EntryField {
            name: "Public".into(),
            value: ProtectedString::new_plain("public-value"),
            is_protected: false,
        },
        EntryField {
            name: "Secret".into(),
            value: ProtectedString::new_protected("custom-secret"),
            is_protected: true,
        },
    ];
    root_entry.binaries.push(EntryBinary {
        name: "secret.bin".into(),
        data: b"attachment-secret".to_vec(),
        is_protected: true,
    });

    let mut child_entry = Entry::new(child_entry_id);
    child_entry.title = "protected-title-secret".into();
    child_entry.title_is_protected = true;
    child_entry.url = "protected-url-secret".into();
    child_entry.url_is_protected = true;
    child_entry.tags = vec!["shared".into()];
    child_entry.custom_icon_uuid = Some(custom_icon_uuid);

    let mut nested_entry = Entry::new(nested_entry_id);
    nested_entry.title = "Nested Entry".into();
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
    (core, ids)
}

fn schema_id(id: Uuid) -> DatabaseNodeId {
    DatabaseNodeId::Uuid(id.hyphenated().to_string())
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
    operations::open::run(
        &mut core,
        StorageDescriptor {
            provider: "memory".into(),
            path: "vault.kdbx".into(),
        },
    )
    .await
    .unwrap();
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
    core.sync(Some(b"correct")).await.unwrap();
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

    let entries = operations::get_entries::run(&mut core).unwrap().entries;
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

    let hierarchy = operations::get_group_hierarchy::run(&mut core).unwrap();
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
        operations::get_entries::run(&mut core),
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
