use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

use super::*;

async fn passkey_core(storage: Arc<MemoryStorage>) -> KeelessCore {
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
    core
}

async fn dispatch_json(core: &mut KeelessCore, request: serde_json::Value) -> serde_json::Value {
    let payload = serde_json::to_vec(&request).unwrap();
    let response = core.handle_payload(&payload).await.unwrap().unwrap();
    serde_json::from_slice(&response).unwrap()
}

fn register_request(request_id: &str, user_name: &str) -> serde_json::Value {
    serde_json::json!({
        "requestId": request_id,
        "op": "registerPasskey",
        "args": {
            "rpId": "example.com",
            "rpName": "Example",
            "userName": user_name,
            "userHandle": URL_SAFE_NO_PAD.encode(user_name.as_bytes()),
            "clientDataHash": URL_SAFE_NO_PAD.encode([7u8; 32]),
            "algorithms": [-7],
            "excludeCredentialIds": [],
        },
    })
}

#[tokio::test]
async fn passkey_operations_register_enumerate_and_assert() {
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(database_bytes(b"correct")))));
    let mut core = passkey_core(storage).await;

    let registered = dispatch_json(&mut core, register_request("request-1", "alice")).await;
    assert_eq!(registered["status"], "success", "{registered}");
    let entry_id = registered["result"]["entryId"]
        .as_str()
        .unwrap()
        .to_string();
    let credential_id = registered["result"]["credentialId"].as_str().unwrap();
    let authenticator_data = URL_SAFE_NO_PAD
        .decode(registered["result"]["authenticatorData"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        &authenticator_data[..32],
        Sha256::digest(b"example.com").as_slice()
    );
    assert_eq!(authenticator_data[32], 0x5d);

    let detail = operations::get_entry_detail::run(
        &mut core,
        GetEntryDetailArgs {
            entry_id: DatabaseNodeId::Uuid(entry_id.clone()),
        },
    )
    .unwrap();
    let named = |name: &str| {
        detail
            .fields
            .iter()
            .filter_map(detail_field)
            .find(|field| field.name == name)
            .unwrap_or_else(|| panic!("entry should contain {name}"))
            .value
            .map(str::to_owned)
    };
    assert_eq!(named("Title"), Some("Example".into()));
    assert_eq!(named("UserName"), Some("alice".into()));
    assert_eq!(named("URL"), Some("https://example.com".into()));
    assert_eq!(
        named("KPEX_PASSKEY_RELYING_PARTY"),
        Some("example.com".into())
    );
    assert_eq!(named("KPEX_PASSKEY_CREDENTIAL_ID"), None, "protected field");

    dispatch_json(&mut core, register_request("request-2", "bob")).await;
    let listed = dispatch_json(
        &mut core,
        serde_json::json!({
            "requestId": "request-3",
            "op": "getPasskeys",
            "args": { "rpId": "example.com" },
        }),
    )
    .await;
    let credentials = listed["result"]["credentials"].as_array().unwrap();
    assert_eq!(credentials.len(), 2);
    let mut usernames = credentials
        .iter()
        .map(|credential| credential["username"].as_str().unwrap())
        .collect::<Vec<_>>();
    usernames.sort_unstable();
    assert_eq!(usernames, ["alice", "bob"]);
    let summary = credentials
        .iter()
        .find(|credential| credential["username"] == "alice")
        .expect("registered credential should be listed");
    assert_eq!(summary["entryId"], entry_id);
    assert_eq!(summary["rpId"], "example.com");

    let other_rp = dispatch_json(
        &mut core,
        serde_json::json!({
            "requestId": "request-4",
            "op": "getPasskeys",
            "args": { "rpId": "example.net" },
        }),
    )
    .await;
    assert!(
        other_rp["result"]["credentials"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let asserted = dispatch_json(
        &mut core,
        serde_json::json!({
            "requestId": "request-5",
            "op": "assertPasskey",
            "args": {
                "rpId": "example.com",
                "clientDataHash": URL_SAFE_NO_PAD.encode([9u8; 32]),
                "allowCredentialIds": [credential_id],
                "userPresent": true,
            },
        }),
    )
    .await;
    assert_eq!(asserted["status"], "success", "{asserted}");
    assert_eq!(asserted["result"]["credentialId"], credential_id);
    assert_eq!(
        asserted["result"]["userHandle"],
        URL_SAFE_NO_PAD.encode(b"alice").as_str()
    );
    assert_eq!(asserted["result"]["userName"], "alice");
    assert!(!asserted["result"]["userSelected"].as_bool().unwrap());
    let assertion_data = URL_SAFE_NO_PAD
        .decode(asserted["result"]["authenticatorData"].as_str().unwrap())
        .unwrap();
    assert_eq!(assertion_data.len(), 37);
    assert_eq!(assertion_data[32], 0x1d);
    assert!(
        !URL_SAFE_NO_PAD
            .decode(asserted["result"]["signature"].as_str().unwrap())
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_silent_assertion_signs_without_the_user_presence_flag() {
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(database_bytes(b"correct")))));
    let mut core = passkey_core(storage).await;
    let registered = dispatch_json(&mut core, register_request("request-1", "alice")).await;
    let credential_id = registered["result"]["credentialId"]
        .as_str()
        .unwrap()
        .to_string();

    let asserted = dispatch_json(
        &mut core,
        serde_json::json!({
            "requestId": "request-2",
            "op": "assertPasskey",
            "args": {
                "rpId": "example.com",
                "clientDataHash": URL_SAFE_NO_PAD.encode([9u8; 32]),
                "allowCredentialIds": [credential_id],
                "userPresent": false,
            },
        }),
    )
    .await;
    assert_eq!(asserted["status"], "success", "{asserted}");
    let authenticator_data = URL_SAFE_NO_PAD
        .decode(asserted["result"]["authenticatorData"].as_str().unwrap())
        .unwrap();
    assert_eq!(authenticator_data[32], 0x18);
}

#[tokio::test]
async fn consent_decline_and_silent_locked_assertion_do_not_sign() {
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(database_bytes(b"correct")))));
    let mut core = passkey_core(storage).await;
    let registered = dispatch_json(&mut core, register_request("request-1", "alice")).await;
    let credential_id = registered["result"]["credentialId"]
        .as_str()
        .unwrap()
        .to_string();
    let consent = Arc::new(PasskeyConsent::cancelled());
    core.passkey_consent = Some(consent.clone());

    let denied = dispatch_json(
        &mut core,
        serde_json::json!({
            "requestId": "request-2",
            "op": "assertPasskey",
            "args": {
                "rpId": "example.com",
                "clientDataHash": URL_SAFE_NO_PAD.encode([9u8; 32]),
                "allowCredentialIds": [credential_id],
                "userPresent": true,
            },
        }),
    )
    .await;
    assert_eq!(denied["error"]["code"], "passkey_consent_denied");
    assert_eq!(consent.requests.lock().unwrap().len(), 1);

    core.credential = None;
    consent.requests.lock().unwrap().clear();
    let silent = dispatch_json(
        &mut core,
        serde_json::json!({
            "requestId": "request-3",
            "op": "assertPasskey",
            "args": {
                "rpId": "example.com",
                "clientDataHash": URL_SAFE_NO_PAD.encode([9u8; 32]),
                "allowCredentialIds": [credential_id],
                "userPresent": false,
            },
        }),
    )
    .await;
    assert_eq!(silent["error"]["code"], "database_locked");
    assert!(consent.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn passkey_operations_reject_invalid_requests() {
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(database_bytes(b"correct")))));
    let mut core = passkey_core(storage).await;

    let registered = dispatch_json(&mut core, register_request("request-1", "alice")).await;
    let credential_id = registered["result"]["credentialId"]
        .as_str()
        .unwrap()
        .to_string();

    let excluded = dispatch_json(
        &mut core,
        serde_json::json!({
            "requestId": "request-2",
            "op": "registerPasskey",
            "args": {
                "rpId": "example.com",
                "userName": "alice",
                "userHandle": URL_SAFE_NO_PAD.encode(b"alice"),
                "clientDataHash": URL_SAFE_NO_PAD.encode([7u8; 32]),
                "algorithms": [-7],
                "excludeCredentialIds": [credential_id],
            },
        }),
    )
    .await;
    assert_eq!(excluded["error"]["code"], "passkey_excluded");

    let unsupported = dispatch_json(
        &mut core,
        serde_json::json!({
            "requestId": "request-3",
            "op": "registerPasskey",
            "args": {
                "rpId": "example.com",
                "userName": "alice",
                "userHandle": URL_SAFE_NO_PAD.encode(b"alice"),
                "clientDataHash": URL_SAFE_NO_PAD.encode([7u8; 32]),
                "algorithms": [-36],
                "excludeCredentialIds": [],
            },
        }),
    )
    .await;
    assert_eq!(
        unsupported["error"]["code"],
        "passkey_unsupported_algorithm"
    );

    let short_hash = dispatch_json(
        &mut core,
        serde_json::json!({
            "requestId": "request-4",
            "op": "assertPasskey",
            "args": {
                "rpId": "example.com",
                "clientDataHash": URL_SAFE_NO_PAD.encode([9u8; 16]),
                "userPresent": true,
            },
        }),
    )
    .await;
    assert_eq!(short_hash["error"]["code"], "invalid_passkey_request");

    let wrong_rp = dispatch_json(
        &mut core,
        serde_json::json!({
            "requestId": "request-5",
            "op": "assertPasskey",
            "args": {
                "rpId": "example.net",
                "clientDataHash": URL_SAFE_NO_PAD.encode([9u8; 32]),
                "userPresent": true,
            },
        }),
    )
    .await;
    assert_eq!(wrong_rp["error"]["code"], "passkey_not_found");

    operations::lock::run(&mut core);
    for op in ["getPasskeys", "registerPasskey", "assertPasskey"] {
        let args = match op {
            "getPasskeys" => serde_json::json!({}),
            "registerPasskey" => register_request("locked", "alice")["args"].clone(),
            _ => serde_json::json!({
                "rpId": "example.com",
                "clientDataHash": URL_SAFE_NO_PAD.encode([9u8; 32]),
                "userPresent": true,
            }),
        };
        let response = dispatch_json(
            &mut core,
            serde_json::json!({ "requestId": "locked", "op": op, "args": args }),
        )
        .await;
        let expected = if op == "getPasskeys" {
            "database_locked"
        } else {
            "password_required"
        };
        assert_eq!(response["error"]["code"], expected, "{op}");
    }
}

#[tokio::test]
async fn registered_passkey_survives_journal_replay() {
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(database_bytes(b"correct")))));
    let persistence = Arc::new(MemoryDatabasePersistence::default());
    let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
    providers.insert("memory".into(), storage.clone());
    let mut core = KeelessCore::new(KeelessHost {
        storage_providers: providers.clone(),
        database_persistence: Some(persistence.clone()),
        ..host(
            Arc::new(MemoryConfig::default()),
            Arc::new(Approval),
            Arc::new(FakeClock::new(100)),
        )
    })
    .await
    .unwrap();
    let descriptor = StorageDescriptor {
        provider: "memory".into(),
        path: "vault.kdbx".into(),
    };
    operations::open::run(&mut core, descriptor.clone())
        .await
        .unwrap();
    operations::unlock::run(&mut core, b"correct")
        .await
        .unwrap();

    let registered = dispatch_json(&mut core, register_request("request-1", "alice")).await;
    assert_eq!(registered["status"], "success", "{registered}");
    let entry_id = registered["result"]["entryId"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(persistence.journal.lock().unwrap().len(), 1);
    assert_eq!(
        core.handle
            .as_ref()
            .unwrap()
            .database()
            .get_entry(&model_id(DatabaseNodeId::Uuid(entry_id.clone())))
            .unwrap()
            .history_count(),
        0,
        "registering a passkey should not create a blank history entry"
    );
    let mut replayed = KeelessCore::new(KeelessHost {
        storage_providers: providers,
        database_persistence: Some(persistence),
        ..host(
            Arc::new(MemoryConfig::default()),
            Arc::new(Approval),
            Arc::new(FakeClock::new(100)),
        )
    })
    .await
    .unwrap();
    operations::open::run(&mut replayed, descriptor)
        .await
        .unwrap();
    operations::unlock::run(&mut replayed, b"correct")
        .await
        .unwrap();
    assert_eq!(
        replayed
            .handle
            .as_ref()
            .unwrap()
            .database()
            .get_entry(&model_id(DatabaseNodeId::Uuid(entry_id.clone())))
            .unwrap()
            .history_count(),
        0,
        "journal replay should not create a blank history entry"
    );

    let listed = dispatch_json(
        &mut replayed,
        serde_json::json!({
            "requestId": "request-2",
            "op": "getPasskeys",
            "args": {},
        }),
    )
    .await;
    let credentials = listed["result"]["credentials"].as_array().unwrap();
    assert_eq!(credentials.len(), 1, "{listed}");
    assert_eq!(credentials[0]["entryId"], entry_id);

    let asserted = dispatch_json(
        &mut replayed,
        serde_json::json!({
            "requestId": "request-3",
            "op": "assertPasskey",
            "args": {
                "rpId": "example.com",
                "clientDataHash": URL_SAFE_NO_PAD.encode([9u8; 32]),
                "userPresent": true,
            },
        }),
    )
    .await;
    assert_eq!(asserted["status"], "success", "{asserted}");
}

#[tokio::test]
async fn failed_passkey_registration_does_not_create_an_entry_or_index() {
    let storage = Arc::new(MemoryStorage(Mutex::new(Some(database_bytes(b"correct")))));
    let persistence = Arc::new(MemoryDatabasePersistence::default());
    let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
    providers.insert("memory".into(), storage);
    let mut core = KeelessCore::new(KeelessHost {
        storage_providers: providers,
        database_persistence: Some(persistence.clone()),
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

    persistence.fail_append.store(true, Ordering::Relaxed);
    let entries_before = core.handle.as_ref().unwrap().database().entry_count();
    let response = dispatch_json(&mut core, register_request("request-1", "alice")).await;
    assert_eq!(response["status"], "error", "{response}");
    assert_eq!(
        core.handle.as_ref().unwrap().database().entry_count(),
        entries_before
    );
    assert!(persistence.journal.lock().unwrap().is_empty());

    let listed = dispatch_json(
        &mut core,
        serde_json::json!({
            "requestId": "request-2",
            "op": "getPasskeys",
            "args": {},
        }),
    )
    .await;
    assert!(
        listed["result"]["credentials"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
