use std::sync::Arc;

use keeless_lesswire::{Client, Identity, KeyScope};
use keeless_schema::{Operation, OperationRequest, UpgradeArgs};

use super::*;

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
    });
    assert!(matches!(
        operations::upgrade::execute(&mut core, UpgradeArgs {}).await,
        Err(CoreError::DatabaseLocked)
    ));
}
