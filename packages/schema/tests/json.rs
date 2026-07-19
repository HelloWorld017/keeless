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
fn storage_descriptor_operation_supports_unselected_and_selected_storage() {
    assert_roundtrip(
        OperationRequest {
            request_id: "storage-1".into(),
            operation: Operation::GetStorageDescriptor(GetStorageDescriptorArgs {}),
        },
        json!({ "requestId": "storage-1", "op": "getStorageDescriptor", "args": {} }),
    );
    assert_roundtrip(
        OperationResponse {
            request_id: "storage-1".into(),
            outcome: OperationOutcome::Success {
                success: OperationSuccess::GetStorageDescriptor(StorageDescriptorResult {
                    storage: None,
                }),
            },
        },
        json!({
            "requestId": "storage-1",
            "status": "success",
            "op": "getStorageDescriptor",
            "result": { "storage": null }
        }),
    );
    assert_roundtrip(
        OperationResponse {
            request_id: "storage-2".into(),
            outcome: OperationOutcome::Success {
                success: OperationSuccess::GetStorageDescriptor(StorageDescriptorResult {
                    storage: Some(StorageDescriptor {
                        provider: "webdav".into(),
                        path: "vault.kdbx".into(),
                    }),
                }),
            },
        },
        json!({
            "requestId": "storage-2",
            "status": "success",
            "op": "getStorageDescriptor",
            "result": {
                "storage": { "provider": "webdav", "path": "vault.kdbx" }
            }
        }),
    );
}

#[test]
fn database_query_requests_support_uuid_and_integer_node_ids() {
    assert_roundtrip(
        OperationRequest {
            request_id: "group-1".into(),
            operation: Operation::GetGroupEntries(GetGroupEntriesArgs {
                group_id: DatabaseNodeId::Uuid("a1b2c3d4-e5f6-7890-abcd-ef1234567890".into()),
            }),
        },
        json!({
            "requestId": "group-1",
            "op": "getGroupEntries",
            "args": { "groupId": "a1b2c3d4-e5f6-7890-abcd-ef1234567890" }
        }),
    );
    assert_roundtrip(
        OperationRequest {
            request_id: "entry-1".into(),
            operation: Operation::GetEntryDetail(GetEntryDetailArgs {
                entry_id: DatabaseNodeId::Int(42),
            }),
        },
        json!({
            "requestId": "entry-1",
            "op": "getEntryDetail",
            "args": { "entryId": 42 }
        }),
    );
    assert_roundtrip(
        OperationRequest {
            request_id: "move-1".into(),
            operation: Operation::MoveGroup(MoveGroupArgs {
                group_id: DatabaseNodeId::Int(42),
                parent_group_id: DatabaseNodeId::Int(7),
                destination_index: 2,
            }),
        },
        json!({
            "requestId": "move-1",
            "op": "moveGroup",
            "args": { "groupId": 42, "parentGroupId": 7, "destinationIndex": 2 }
        }),
    );
}

#[test]
fn entry_detail_keeps_protected_fields_but_omits_their_values() {
    assert_roundtrip(
        OperationResponse {
            request_id: "entry-2".into(),
            outcome: OperationOutcome::Success {
                success: OperationSuccess::GetEntryDetail(Box::new(EntryDetailResult {
                    id: DatabaseNodeId::Int(7),
                    icon: IconReference {
                        standard_id: 1,
                        custom_uuid: None,
                    },
                    tags: vec!["work".into()],
                    fields: vec![EntryFieldInformation {
                        name: "Password".into(),
                        value: None,
                        is_protected: true,
                    }],
                    background_color: "#000000".into(),
                    foreground_color: "#ffffff".into(),
                    override_url: String::new(),
                    creation_time_ms: Some(100),
                    last_modification_time_ms: Some(200),
                    last_access_time_ms: Some(300),
                    location_changed_ms: Some(400),
                    expires: false,
                    expiry_time_ms: None,
                    usage_count: 2,
                    attachments: vec![EntryAttachmentInformation {
                        name: "key.txt".into(),
                        size: 12,
                        is_protected: true,
                    }],
                })),
            },
        },
        json!({
            "requestId": "entry-2",
            "status": "success",
            "op": "getEntryDetail",
            "result": {
                "id": 7,
                "icon": { "standardId": 1, "customUuid": null },
                "tags": ["work"],
                "fields": [{ "name": "Password", "value": null, "isProtected": true }],
                "backgroundColor": "#000000",
                "foregroundColor": "#ffffff",
                "overrideUrl": "",
                "creationTimeMs": 100,
                "lastModificationTimeMs": 200,
                "lastAccessTimeMs": 300,
                "locationChangedMs": 400,
                "expires": false,
                "expiryTimeMs": null,
                "usageCount": 2,
                "attachments": [{ "name": "key.txt", "size": 12, "isProtected": true }]
            }
        }),
    );
}

#[test]
fn custom_icons_serialize_base64_data_and_metadata() {
    assert_roundtrip(
        OperationResponse {
            request_id: "icons-1".into(),
            outcome: OperationOutcome::Success {
                success: OperationSuccess::GetCustomIcons(CustomIconsResult {
                    icons: vec![CustomIcon {
                        uuid: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".into(),
                        data_base64: "AQID".into(),
                        name: "Custom".into(),
                        last_modification_time_ms: 123,
                    }],
                }),
            },
        },
        json!({
            "requestId": "icons-1",
            "status": "success",
            "op": "getCustomIcons",
            "result": {
                "icons": [{
                    "uuid": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                    "dataBase64": "AQID",
                    "name": "Custom",
                    "lastModificationTimeMs": 123
                }]
            }
        }),
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
    assert!(serde_json::from_value::<StorageDescriptorResult>(json!({})).is_err());
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
        json!({ "requestId": "5", "op": "getEntries", "args": {} }),
        json!({ "requestId": "6", "op": "getGroupHierarchy", "args": {} }),
        json!({ "requestId": "7", "op": "getTags", "args": {} }),
        json!({ "requestId": "8", "op": "getCustomIcons", "args": {} }),
    ];
    for value in requests {
        let parsed: OperationRequest = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }

    for operation in ["open", "create", "unlock", "lock", "setConfig", "moveGroup"] {
        let value = json!({ "requestId": "5", "status": "success", "op": operation, "result": {} });
        let parsed: OperationResponse = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }
}
