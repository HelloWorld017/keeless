use std::sync::Arc;

use keeless_lesswire::{Client, Identity, KeyScope};
use keeless_schema::{LockArgs, Operation, OperationRequest, UpgradeArgs};

use super::*;

struct DenyApproval;

impl ConnectionApprovalProvider for DenyApproval {
    fn approve_connection(&self, _: ConnectionApprovalRequest) -> HostFuture<'_, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
}

#[tokio::test]
async fn untrusted_endpoint_drops_core_operations_before_execution() {
    let clock = Arc::new(FakeClock::new(10_000));
    let core = KeelessCore::new(host(
        Arc::new(MemoryConfig::default()),
        Arc::new(Approval),
        clock.clone(),
    ))
    .await
    .unwrap();
    let recipient = core.untrusted_public_key_bundle();
    let mut client = Client::new(
        Identity::from_secrets(KeyScope::App, [90; 32], [91; 32]),
        &recipient,
        clock,
    )
    .unwrap();
    let mut core = core;
    let handshake = client.handshake_frame().unwrap();
    let response = core
        .handle_frame(&serde_json::to_vec(&handshake).unwrap())
        .await
        .unwrap()
        .unwrap();
    client
        .accept_handshake(&serde_json::from_slice(&response).unwrap())
        .unwrap();
    let payload = serde_json::to_vec(&OperationRequest {
        request_id: "request".into(),
        operation: Operation::Lock(keeless_schema::LockArgs { password: None }),
    })
    .unwrap();
    let request = client.encrypt(&payload).unwrap();
    assert!(
        core.handle_frame(&serde_json::to_vec(&request).unwrap())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        operations::get_database_status::run(&mut core),
        DatabaseStatus::NotExist
    );
}

#[tokio::test]
async fn upgrade_reports_database_locked_without_active_core_server() {
    let mut core = KeelessCore::new(host(
        Arc::new(MemoryConfig::default()),
        Arc::new(Approval),
        Arc::new(FakeClock::new(10_000)),
    ))
    .await
    .unwrap();
    core.authenticated_sender = Some(keeless_lesswire::AuthenticatedSender {
        public_key_bundle: Identity::from_secrets(KeyScope::App, [92; 32], [93; 32])
            .public_key_bundle(),
        scope: KeyScope::App,
        approval: keeless_lesswire::SenderApproval::Persisted,
    });
    assert!(matches!(
        operations::upgrade::execute(&mut core, UpgradeArgs {}).await,
        Err(CoreError::DatabaseLocked)
    ));
}

async fn handshake(core: &mut KeelessCore, client: &mut Client) {
    let response = core
        .handle_frame(&serde_json::to_vec(&client.handshake_frame().unwrap()).unwrap())
        .await
        .unwrap()
        .unwrap();
    client
        .accept_handshake(&serde_json::from_slice(&response).unwrap())
        .unwrap();
}

async fn request(core: &mut KeelessCore, client: &Client, operation: Operation) -> Option<Vec<u8>> {
    let request = OperationRequest {
        request_id: crypto_request_id(),
        operation,
    };
    let frame = client
        .encrypt(&serde_json::to_vec(&request).unwrap())
        .unwrap();
    core.handle_frame(&serde_json::to_vec(&frame).unwrap())
        .await
        .unwrap()
}

fn crypto_request_id() -> String {
    "request".into()
}

#[tokio::test]
async fn lock_drops_the_taken_core_server_and_preserves_untrusted_status() {
    let clock = Arc::new(FakeClock::new(10_000));
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(database_bytes(b"correct")))));
    let persistence = Arc::new(MemoryDatabasePersistence::default());
    let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
    providers.insert("memory".into(), storage);
    let mut core = KeelessCore::new(KeelessHost {
        storage_providers: providers,
        database_persistence: persistence,
        ..host(
            Arc::new(MemoryConfig::default()),
            Arc::new(Approval),
            clock.clone(),
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

    let untrusted_recipient = core.untrusted_public_key_bundle();
    let identity = Identity::from_secrets(KeyScope::App, [94; 32], [95; 32]);
    let mut untrusted = Client::new(identity, &untrusted_recipient, clock.clone()).unwrap();
    handshake(&mut core, &mut untrusted).await;
    let upgrade = request(&mut core, &untrusted, Operation::Upgrade(UpgradeArgs {}))
        .await
        .unwrap();
    let upgrade = untrusted
        .decrypt(&serde_json::from_slice(&upgrade).unwrap())
        .unwrap()
        .unwrap();
    let response: serde_json::Value = serde_json::from_slice(&upgrade).unwrap();
    let core_recipient = response["result"]["publicKey"].as_str().unwrap();
    let mut database = Client::new(
        Identity::from_secrets(KeyScope::App, [94; 32], [95; 32]),
        core_recipient,
        clock,
    )
    .unwrap();
    handshake(&mut core, &mut database).await;

    assert!(
        request(
            &mut core,
            &database,
            Operation::Lock(LockArgs { password: None })
        )
        .await
        .is_some()
    );
    assert!(core.core_public_key_bundle().is_none());
    assert!(
        request(
            &mut core,
            &database,
            Operation::GetDatabaseStatus(keeless_schema::GetDatabaseStatusArgs {}),
        )
        .await
        .is_none()
    );
    assert!(
        request(
            &mut core,
            &untrusted,
            Operation::GetCoreStatus(keeless_schema::GetCoreStatusArgs {}),
        )
        .await
        .is_some()
    );
}

#[tokio::test]
async fn failed_initial_approval_persistence_restores_the_untrusted_server() {
    let clock = Arc::new(FakeClock::new(10_000));
    let untrusted_state = Arc::new(MemoryConfig::default());
    let mut core = KeelessCore::new(KeelessHost {
        untrusted_state: untrusted_state.clone(),
        ..host(untrusted_state.clone(), Arc::new(Approval), clock.clone())
    })
    .await
    .unwrap();
    let recipient = core.untrusted_public_key_bundle();
    let mut client = Client::new(
        Identity::from_secrets(KeyScope::App, [96; 32], [97; 32]),
        &recipient,
        clock,
    )
    .unwrap();
    untrusted_state.fail_save.store(true, Ordering::Relaxed);
    let response = core
        .handle_frame(&serde_json::to_vec(&client.handshake_frame().unwrap()).unwrap())
        .await;
    assert!(matches!(response, Err(CoreError::Host(_))));
    assert_eq!(core.untrusted_public_key_bundle(), recipient);
    untrusted_state.fail_save.store(false, Ordering::Relaxed);
    handshake(&mut core, &mut client).await;
}

#[tokio::test]
async fn runtime_approved_client_skips_initial_and_upgrade_prompts() {
    let clock = Arc::new(FakeClock::new(10_000));
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(database_bytes(b"correct")))));
    let persistence = Arc::new(MemoryDatabasePersistence::default());
    let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
    providers.insert("memory".into(), storage);
    let mut core = KeelessCore::new(KeelessHost {
        storage_providers: providers,
        database_persistence: persistence,
        connection_approval: Arc::new(DenyApproval),
        ..host(
            Arc::new(MemoryConfig::default()),
            Arc::new(Approval),
            clock.clone(),
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

    let identity = Identity::from_secrets(KeyScope::App, [98; 32], [99; 32]);
    core.untrusted_server
        .as_mut()
        .unwrap()
        .add_runtime_approval(&identity.public_key_bundle())
        .unwrap();
    let recipient = core.untrusted_public_key_bundle();
    let mut client = Client::new(identity, &recipient, clock).unwrap();
    handshake(&mut core, &mut client).await;
    let upgrade = request(&mut core, &client, Operation::Upgrade(UpgradeArgs {}))
        .await
        .unwrap();
    let upgrade = client
        .decrypt(&serde_json::from_slice(&upgrade).unwrap())
        .unwrap()
        .unwrap();
    let response: serde_json::Value = serde_json::from_slice(&upgrade).unwrap();
    let core_recipient = response["result"]["publicKey"].as_str().unwrap();
    let mut database = Client::new(
        Identity::from_secrets(KeyScope::App, [98; 32], [99; 32]),
        core_recipient,
        Arc::new(FakeClock::new(10_000)),
    )
    .unwrap();
    handshake(&mut core, &mut database).await;
}
