use super::*;

#[tokio::test]
async fn config_patch_is_deep_and_paranoia_is_persisted() {
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(database_bytes(b"correct")))));
    let persistence = Arc::new(MemoryDatabasePersistence::default());
    let (mut core, _) = query_core_with_persistence(storage, persistence.clone()).await;
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
    assert!(persistence.state.lock().unwrap().contains_key("config"));
}

#[tokio::test]
async fn framed_payload_dispatches_and_rejects_invalid_requests() {
    use keeless_lesswire::{Client, Identity, KeyScope};

    let clock = Arc::new(FakeClock::new(10_000));
    let mut core = KeelessCore::new(host(
        Arc::new(MemoryConfig::default()),
        Arc::new(Approval),
        clock.clone(),
    ))
    .await
    .unwrap();
    let recipient = core.untrusted_public_key_bundle();
    let mut client = Client::new(
        Identity::from_secrets(KeyScope::App, [81; 32], [82; 32]),
        &recipient,
        clock,
    )
    .unwrap();
    let handshake = core
        .handle_frame(&serde_json::to_vec(&client.handshake_frame().unwrap()).unwrap())
        .await
        .unwrap()
        .unwrap();
    client
        .accept_handshake(&serde_json::from_slice(&handshake).unwrap())
        .unwrap();
    let frame = client
        .encrypt(br#"{"requestId":"request-1","op":"getCoreStatus","args":{}}"#)
        .unwrap();
    let response = core
        .handle_frame(&serde_json::to_vec(&frame).unwrap())
        .await
        .unwrap()
        .unwrap();
    let response = client
        .decrypt(&serde_json::from_slice(&response).unwrap())
        .unwrap()
        .unwrap();
    let response: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(response["requestId"], "request-1");
    assert_eq!(response["status"], "success");
    assert_eq!(response["result"]["database"], "not_exist");
    assert!(core.handle_frame(b"not json").await.unwrap().is_none());
}

#[tokio::test]
async fn failed_persistence_rolls_back_settings() {
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(database_bytes(b"correct")))));
    let persistence = Arc::new(MemoryDatabasePersistence::default());
    let (mut core, _) = query_core_with_persistence(storage, persistence.clone()).await;
    persistence.fail_state_write.store(true, Ordering::Relaxed);

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
