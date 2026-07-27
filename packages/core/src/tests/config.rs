use super::*;

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
