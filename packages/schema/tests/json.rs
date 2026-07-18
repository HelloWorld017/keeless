use keeless_schema::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn assert_roundtrip<T>(value: T, expected: Value)
where
    T: Serialize + DeserializeOwned + PartialEq,
{
    assert_eq!(serde_json::to_value(&value).unwrap(), expected);
    assert!(serde_json::from_value::<T>(expected).unwrap() == value);
}

#[test]
fn message_frame_uses_camel_case_and_nullable_fields() {
    assert_roundtrip(
        MessageFrame {
            version: 1,
            timestamp: 123,
            nonce: "nonce".into(),
            ephemeral_public_key: None,
            public_key: "public".into(),
            payload: Some("ciphertext".into()),
            signature: "signature".into(),
        },
        json!({
            "version": 1,
            "timestamp": 123,
            "nonce": "nonce",
            "ephemeralPublicKey": null,
            "publicKey": "public",
            "payload": "ciphertext",
            "signature": "signature"
        }),
    );
}

#[test]
fn config_defaults_and_patch_preserve_three_states() {
    assert_eq!(
        KeelessConfig::default(),
        KeelessConfig {
            auto_lock_timeout_ms: None,
            paranoia_mode: false,
        }
    );

    let absent: KeelessConfigPatch = serde_json::from_value(json!({})).unwrap();
    let null: KeelessConfigPatch =
        serde_json::from_value(json!({ "autoLockTimeoutMs": null })).unwrap();
    let value: KeelessConfigPatch = serde_json::from_value(json!({
        "autoLockTimeoutMs": 30000,
        "paranoiaMode": true
    }))
    .unwrap();

    assert_eq!(absent.auto_lock_timeout_ms, None);
    assert_eq!(null.auto_lock_timeout_ms, Some(None));
    assert_eq!(value.auto_lock_timeout_ms, Some(Some(30_000)));
    assert_eq!(serde_json::to_value(absent).unwrap(), json!({}));
    assert_eq!(
        serde_json::to_value(null).unwrap(),
        json!({ "autoLockTimeoutMs": null })
    );
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        json!({ "autoLockTimeoutMs": 30000, "paranoiaMode": true })
    );
}

#[test]
fn request_is_flat_with_stable_op_and_args() {
    let json = json!({
        "requestId": "request-1",
        "op": "unlock",
        "args": { "password": "secret" }
    });
    let request: OperationRequest = serde_json::from_value(json.clone()).unwrap();
    assert!(matches!(request.operation, Operation::Unlock(_)));
    assert_eq!(serde_json::to_value(request).unwrap(), json);

    assert_roundtrip(
        OperationRequest {
            request_id: "request-2".into(),
            operation: Operation::GetDatabaseStatus(GetDatabaseStatusArgs {}),
        },
        json!({ "requestId": "request-2", "op": "getDatabaseStatus", "args": {} }),
    );
}

#[test]
fn create_request_and_response_have_stable_shapes() {
    assert_roundtrip(
        OperationRequest {
            request_id: "create-1".into(),
            operation: Operation::Create(CreateArgs {
                password: "secret".into(),
            }),
        },
        json!({ "requestId": "create-1", "op": "create", "args": { "password": "secret" } }),
    );
    assert_roundtrip(
        OperationResponse {
            request_id: "create-1".into(),
            outcome: OperationOutcome::Success {
                success: OperationSuccess::Create(EmptyResult {}),
            },
        },
        json!({ "requestId": "create-1", "status": "success", "op": "create", "result": {} }),
    );
}

#[test]
fn success_response_is_flat_and_operation_specific() {
    assert_roundtrip(
        OperationResponse {
            request_id: "request-3".into(),
            outcome: OperationOutcome::Success {
                success: OperationSuccess::GetDatabaseStatus(DatabaseStatusResult {
                    status: DatabaseStatus::NotExist,
                }),
            },
        },
        json!({
            "requestId": "request-3",
            "status": "success",
            "op": "getDatabaseStatus",
            "result": { "status": "not_exist" }
        }),
    );
}

#[test]
fn required_nullable_and_empty_types_reject_ambiguous_shapes() {
    let missing_ephemeral = json!({
        "version": 1,
        "timestamp": 123,
        "nonce": "nonce",
        "publicKey": "public",
        "payload": null,
        "signature": "signature"
    });
    assert!(serde_json::from_value::<MessageFrame>(missing_ephemeral).is_err());
    assert!(serde_json::from_value::<LockArgs>(json!({ "ignored": true })).is_err());
}

#[test]
fn error_response_is_flat_and_excludes_success() {
    assert_roundtrip(
        OperationResponse {
            request_id: "request-4".into(),
            outcome: OperationOutcome::Error {
                error: OperationError {
                    code: "invalidPassword".into(),
                    message: "The password is invalid".into(),
                },
            },
        },
        json!({
            "requestId": "request-4",
            "status": "error",
            "error": {
                "code": "invalidPassword",
                "message": "The password is invalid"
            }
        }),
    );
}

#[test]
fn all_request_and_success_variants_have_explicit_empty_objects() {
    let requests = [
        json!({ "requestId": "1", "op": "open", "args": { "storage": { "provider": "file", "path": "/vault.kdbx" } } }),
        json!({ "requestId": "2", "op": "lock", "args": {} }),
        json!({ "requestId": "3", "op": "getConfig", "args": {} }),
        json!({ "requestId": "4", "op": "setConfig", "args": { "config": { "paranoiaMode": true } } }),
    ];
    for value in requests {
        let parsed: OperationRequest = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }

    for operation in ["open", "create", "unlock", "lock", "setConfig"] {
        let value = json!({ "requestId": "5", "status": "success", "op": operation, "result": {} });
        let parsed: OperationResponse = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }
}
