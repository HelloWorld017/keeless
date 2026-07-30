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
fn operations_expose_query_fetch_and_mutation_metadata() {
    let get_tags = Operation::GetTags(GetTagsArgs {}).metadata();
    assert_eq!(get_tags.queries, &[OperationResource::Tag]);
    assert!(get_tags.fetches.is_empty());
    assert!(get_tags.mutates.is_empty());

    let update_tag = Operation::UpdateTagStyle(UpdateTagStyleArgs {
        name: "Work".into(),
        style: TagStyle {
            icon: IconReference {
                standard_id: 0,
                custom_uuid: None,
            },
            color: "blue".into(),
        },
    })
    .metadata();
    assert!(update_tag.queries.is_empty());
    assert!(update_tag.fetches.is_empty());
    assert_eq!(
        update_tag.mutates,
        &[OperationResource::DatabaseStatus, OperationResource::Tag]
    );

    let fetches = Operation::definitions()
        .iter()
        .filter(|definition| !definition.metadata.fetches.is_empty())
        .map(|definition| (definition.name, definition.metadata.fetches))
        .collect::<Vec<_>>();
    assert_eq!(
        fetches,
        vec![
            (
                "prepareEntryAttachmentDownload",
                &[OperationResource::Entry][..]
            ),
            (
                "prepareDatabaseExport",
                &[
                    OperationResource::Entry,
                    OperationResource::Group,
                    OperationResource::Tag,
                    OperationResource::CustomIcon,
                ][..],
            ),
            ("revealEntryFields", &[OperationResource::Entry][..]),
            ("assertPasskey", &[OperationResource::Entry][..]),
        ]
    );

    assert!(
        Operation::definitions()
            .iter()
            .any(|definition| definition.name == "getTags" && definition.metadata == get_tags)
    );
}

#[test]
fn create_request_and_response_have_stable_shapes() {
    assert_roundtrip(
        OperationRequest {
            request_id: "create-1".into(),
            operation: Operation::Create(CreateArgs {
                password: Some("secret".into()),
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
fn create_unlock_and_lock_passwords_are_optional_and_nullable() {
    for (operation, expected) in [
        (
            Operation::Create(CreateArgs { password: None }),
            json!({ "op": "create", "args": { "password": null } }),
        ),
        (
            Operation::Unlock(UnlockArgs { password: None }),
            json!({ "op": "unlock", "args": { "password": null } }),
        ),
        (
            Operation::Lock(LockArgs { password: None }),
            json!({ "op": "lock", "args": { "password": null } }),
        ),
    ] {
        assert_roundtrip(operation, expected);
    }

    assert!(
        serde_json::from_value::<CreateArgs>(json!({})).unwrap() == CreateArgs { password: None }
    );
    assert!(
        serde_json::from_value::<UnlockArgs>(json!({})).unwrap() == UnlockArgs { password: None }
    );
    assert!(serde_json::from_value::<LockArgs>(json!({})).unwrap() == LockArgs { password: None });
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
        OperationRequest {
            request_id: "fuzzy-1".into(),
            operation: Operation::SearchFuzzy(SearchFuzzyArgs {
                query: "tag:work pass".into(),
            }),
        },
        json!({
            "requestId": "fuzzy-1",
            "op": "searchFuzzy",
            "args": { "query": "tag:work pass" }
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
    assert_roundtrip(
        OperationResponse {
            request_id: "fuzzy-2".into(),
            outcome: OperationOutcome::Success {
                success: OperationSuccess::SearchFuzzy(SearchFuzzyResult {
                    entries: vec![],
                    groups: vec![],
                    tags: vec![],
                    trash_matches: false,
                    filter_tokens: vec!["tag:work".into()],
                }),
            },
        },
        json!({
            "requestId": "fuzzy-2",
            "status": "success",
            "op": "searchFuzzy",
            "result": {
                "entries": [],
                "groups": [],
                "tags": [],
                "trashMatches": false,
                "filterTokens": ["tag:work"]
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
    assert_roundtrip(
        OperationRequest {
            request_id: "tag-1".into(),
            operation: Operation::GetTagEntries(GetTagEntriesArgs { tag: "Work".into() }),
        },
        json!({ "requestId": "tag-1", "op": "getTagEntries", "args": { "tag": "Work" } }),
    );
    assert_roundtrip(
        OperationRequest {
            request_id: "search-1".into(),
            operation: Operation::SearchEntries(SearchEntriesArgs {
                query: "example account".into(),
            }),
        },
        json!({
            "requestId": "search-1",
            "op": "searchEntries",
            "args": { "query": "example account" }
        }),
    );
    assert_roundtrip(
        OperationRequest {
            request_id: "trash-1".into(),
            operation: Operation::GetTrashEntries(GetTrashEntriesArgs {}),
        },
        json!({ "requestId": "trash-1", "op": "getTrashEntries", "args": {} }),
    );
    assert_roundtrip(
        OperationRequest {
            request_id: "move-entry-1".into(),
            operation: Operation::MoveEntry(MoveEntryArgs {
                entry_id: DatabaseNodeId::Int(42),
                parent_group_id: DatabaseNodeId::Int(7),
            }),
        },
        json!({
            "requestId": "move-entry-1",
            "op": "moveEntry",
            "args": { "entryId": 42, "parentGroupId": 7 }
        }),
    );
}

#[test]
fn entry_edit_requests_preserve_nullable_field_and_password_shapes() {
    assert_roundtrip(
        OperationRequest {
            request_id: "update-1".into(),
            operation: Operation::UpdateEntry(UpdateEntryArgs {
                entry_id: DatabaseNodeId::Int(42),
                fields: vec![
                    EntryFieldUpdate {
                        field_id: Some("standard:Password".into()),
                        name: "Password".into(),
                        value: None,
                        is_protected: true,
                    },
                    EntryFieldUpdate {
                        field_id: None,
                        name: "Account".into(),
                        value: Some("value".into()),
                        is_protected: false,
                    },
                ],
                properties: None,
                attachments: None,
                removed_attachment_indices: None,
                password: None,
            }),
        },
        json!({
            "requestId": "update-1",
            "op": "updateEntry",
            "args": {
                "entryId": 42,
                "password": null,
                "properties": null,
                "fields": [
                    { "fieldId": "standard:Password", "name": "Password", "value": null, "isProtected": true },
                    { "fieldId": null, "name": "Account", "value": "value", "isProtected": false }
                ]
            }
        }),
    );
    assert_roundtrip(
        Operation::DeleteEntry(DeleteEntryArgs {
            entry_id: DatabaseNodeId::Int(42),
            permanent: false,
        }),
        json!({ "op": "deleteEntry", "args": { "entryId": 42, "permanent": false } }),
    );
    assert_roundtrip(
        Operation::EmptyRecycleBin(EmptyRecycleBinArgs {}),
        json!({ "op": "emptyRecycleBin", "args": {} }),
    );
    assert_roundtrip(
        Operation::PrepareEntryAttachmentDownload(PrepareEntryAttachmentDownloadArgs {
            entry_id: DatabaseNodeId::Int(42),
            attachment_index: 3,
            name: "document.pdf".into(),
        }),
        json!({
            "op": "prepareEntryAttachmentDownload",
            "args": { "entryId": 42, "attachmentIndex": 3, "name": "document.pdf" }
        }),
    );
    assert_roundtrip(
        Operation::SaveDatabase(SaveDatabaseArgs {
            password: Some("secret".into()),
        }),
        json!({ "op": "saveDatabase", "args": { "password": "secret" } }),
    );
}

#[allow(clippy::bool_assert_comparison)]
#[test]
fn get_entries_excludes_trash_by_default_and_accepts_explicit_false() {
    assert_eq!(GetEntriesArgs::default().exclude_trash, true);
    assert_eq!(
        serde_json::from_value::<GetEntriesArgs>(json!({}))
            .unwrap()
            .exclude_trash,
        true
    );
    assert_roundtrip(
        GetEntriesArgs {
            exclude_trash: false,
        },
        json!({ "excludeTrash": false }),
    );
}

#[test]
fn entry_summaries_include_usernames_and_tags_without_exposing_protected_values() {
    assert_roundtrip(
        EntriesResult {
            entries: vec![
                EntrySummary {
                    id: DatabaseNodeId::Int(1),
                    name: Some("Email".into()),
                    name_is_protected: false,
                    username: Some("alice".into()),
                    username_is_protected: false,
                    url: Some("https://example.test".into()),
                    url_is_protected: false,
                    icon: IconReference {
                        standard_id: 0,
                        custom_uuid: None,
                    },
                    tags: vec!["personal".into(), "email".into()],
                },
                EntrySummary {
                    id: DatabaseNodeId::Int(2),
                    name: None,
                    name_is_protected: true,
                    username: None,
                    username_is_protected: true,
                    url: None,
                    url_is_protected: true,
                    icon: IconReference {
                        standard_id: 1,
                        custom_uuid: None,
                    },
                    tags: Vec::new(),
                },
            ],
        },
        json!({
            "entries": [
                {
                    "id": 1,
                    "name": "Email",
                    "nameIsProtected": false,
                    "username": "alice",
                    "usernameIsProtected": false,
                    "url": "https://example.test",
                    "urlIsProtected": false,
                    "icon": { "standardId": 0, "customUuid": null },
                    "tags": ["personal", "email"]
                },
                {
                    "id": 2,
                    "name": null,
                    "nameIsProtected": true,
                    "username": null,
                    "usernameIsProtected": true,
                    "url": null,
                    "urlIsProtected": true,
                    "icon": { "standardId": 1, "customUuid": null },
                    "tags": []
                }
            ]
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
                    is_template: false,
                    icon: IconReference {
                        standard_id: 1,
                        custom_uuid: None,
                    },
                    tags: vec!["work".into()],
                    fields: vec![EntryFieldInformation::Field {
                        order: 0,
                        field_id: Some("standard:Password".into()),
                        kind: EntryFieldKind::Password,
                        name: "Password".into(),
                        label: "Password".into(),
                        value: None,
                        is_protected: true,
                        is_internal: false,
                        control: None,
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
                        index: 0,
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
                "isTemplate": false,
                "icon": { "standardId": 1, "customUuid": null },
                "tags": ["work"],
                "fields": [{ "type": "field", "order": 0, "fieldId": "standard:Password", "kind": "password", "name": "Password", "label": "Password", "value": null, "isProtected": true, "isInternal": false, "control": null }],
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
                "attachments": [{ "index": 0, "name": "key.txt", "size": 12, "isProtected": true }]
            }
        }),
    );
}

#[test]
fn entry_field_layout_and_entry_properties_roundtrip() {
    assert_roundtrip(
        vec![
            EntryFieldInformation::Field {
                order: 0,
                field_id: Some("field:account-number".into()),
                kind: EntryFieldKind::Custom,
                name: "Account Number".into(),
                label: "Account number".into(),
                value: Some("1234".into()),
                is_protected: false,
                is_internal: false,
                control: FieldControl::Text {
                    protected: false,
                    lines: 1,
                }
                .into(),
            },
            EntryFieldInformation::Field {
                order: 1,
                field_id: None,
                kind: EntryFieldKind::Custom,
                name: "Account Type".into(),
                label: "Account type".into(),
                value: Some(String::new()),
                is_protected: false,
                is_internal: false,
                control: Some(FieldControl::Select {
                    options: vec!["Checking".into(), "Savings".into()],
                }),
            },
            EntryFieldInformation::PasswordConfirmation {
                order: 2,
                label: "Confirm password".into(),
                password_field_id: "standard:Password".into(),
                control: FieldControl::Popout { protected: true },
            },
            EntryFieldInformation::OverrideUrl {
                order: 3,
                label: "Website".into(),
                control: FieldControl::Url,
            },
            EntryFieldInformation::Expiry {
                order: 4,
                label: "Expires".into(),
                control: FieldControl::DateTime,
            },
            EntryFieldInformation::Tags {
                order: 5,
                label: "Tags".into(),
                control: FieldControl::RichText { lines: 2 },
            },
            EntryFieldInformation::Divider {
                order: 6,
                label: String::new(),
            },
        ],
        json!([
            {
                "type": "field",
                "order": 0,
                "fieldId": "field:account-number",
                "kind": "custom",
                "name": "Account Number",
                "label": "Account number",
                "value": "1234",
                "isProtected": false,
                "isInternal": false,
                "control": { "type": "text", "protected": false, "lines": 1 }
            },
            {
                "type": "field",
                "order": 1,
                "fieldId": null,
                "kind": "custom",
                "name": "Account Type",
                "label": "Account type",
                "value": "",
                "isProtected": false,
                "isInternal": false,
                "control": { "type": "select", "options": ["Checking", "Savings"] }
            },
            {
                "type": "passwordConfirmation",
                "order": 2,
                "label": "Confirm password",
                "passwordFieldId": "standard:Password",
                "control": { "type": "popout", "protected": true }
            },
            {
                "type": "overrideUrl",
                "order": 3,
                "label": "Website",
                "control": { "type": "url" }
            },
            {
                "type": "expiry",
                "order": 4,
                "label": "Expires",
                "control": { "type": "dateTime" }
            },
            {
                "type": "tags",
                "order": 5,
                "label": "Tags",
                "control": { "type": "richText", "lines": 2 }
            },
            {
                "type": "divider",
                "order": 6,
                "label": ""
            }
        ]),
    );

    assert_roundtrip(
        EntryPropertiesUpdate {
            override_url: "https://example.test/login".into(),
            tags: vec!["finance".into(), "personal".into()],
            expires: true,
            expiry_time_ms: Some(1_900_000_000_000),
            icon: None,
        },
        json!({
            "overrideUrl": "https://example.test/login",
            "tags": ["finance", "personal"],
            "expires": true,
            "expiryTimeMs": 1900000000000_i64
        }),
    );
}

#[test]
fn database_editing_operations_have_stable_shapes() {
    let style = TagStyle {
        icon: IconReference {
            standard_id: 12,
            custom_uuid: None,
        },
        color: "#abcdef".into(),
    };
    assert_roundtrip(
        OperationRequest {
            request_id: "tag-style-1".into(),
            operation: Operation::UpdateTagStyle(UpdateTagStyleArgs {
                name: "work".into(),
                style: style.clone(),
            }),
        },
        json!({
            "requestId": "tag-style-1",
            "op": "updateTagStyle",
            "args": {
                "name": "work",
                "style": { "icon": { "standardId": 12, "customUuid": null }, "color": "#abcdef" }
            }
        }),
    );
    assert_roundtrip(
        OperationRequest {
            request_id: "delete-tag-1".into(),
            operation: Operation::DeleteTag(DeleteTagArgs {
                name: "work".into(),
            }),
        },
        json!({ "requestId": "delete-tag-1", "op": "deleteTag", "args": { "name": "work" } }),
    );
    assert_roundtrip(
        OperationRequest {
            request_id: "update-group-1".into(),
            operation: Operation::UpdateGroup(UpdateGroupArgs {
                group_id: DatabaseNodeId::Int(9),
                name: "Personal".into(),
                icon: IconReference {
                    standard_id: 4,
                    custom_uuid: Some("00000000-0000-0000-0000-000000000001".into()),
                },
            }),
        },
        json!({
            "requestId": "update-group-1",
            "op": "updateGroup",
            "args": {
                "groupId": 9,
                "name": "Personal",
                "icon": { "standardId": 4, "customUuid": "00000000-0000-0000-0000-000000000001" }
            }
        }),
    );
    assert_roundtrip(
        OperationRequest {
            request_id: "templates-1".into(),
            operation: Operation::GetEntryTemplates(GetEntryTemplatesArgs {}),
        },
        json!({ "requestId": "templates-1", "op": "getEntryTemplates", "args": {} }),
    );
    assert_roundtrip(
        OperationRequest {
            request_id: "add-entry-1".into(),
            operation: Operation::AddEntry(AddEntryArgs {
                parent_group_id: DatabaseNodeId::Int(7),
            }),
        },
        json!({ "requestId": "add-entry-1", "op": "addEntry", "args": { "parentGroupId": 7 } }),
    );
    assert_roundtrip(
        OperationResponse {
            request_id: "add-entry-1".into(),
            outcome: OperationOutcome::Success {
                success: OperationSuccess::AddEntry(AddEntryResult {
                    id: DatabaseNodeId::Int(8),
                }),
            },
        },
        json!({ "requestId": "add-entry-1", "status": "success", "op": "addEntry", "result": { "id": 8 } }),
    );
    assert_roundtrip(
        OperationRequest {
            request_id: "add-template-1".into(),
            operation: Operation::AddEntryFromTemplate(AddEntryFromTemplateArgs {
                parent_group_id: DatabaseNodeId::Int(7),
                template_entry_id: DatabaseNodeId::Int(4),
            }),
        },
        json!({
            "requestId": "add-template-1",
            "op": "addEntryFromTemplate",
            "args": { "parentGroupId": 7, "templateEntryId": 4 }
        }),
    );
    assert_roundtrip(
        OperationResponse {
            request_id: "add-template-1".into(),
            outcome: OperationOutcome::Success {
                success: OperationSuccess::AddEntryFromTemplate(AddEntryResult {
                    id: DatabaseNodeId::Int(10),
                }),
            },
        },
        json!({
            "requestId": "add-template-1",
            "status": "success",
            "op": "addEntryFromTemplate",
            "result": { "id": 10 }
        }),
    );
    assert_roundtrip(
        OperationRequest {
            request_id: "add-group-1".into(),
            operation: Operation::AddGroup(AddGroupArgs {
                parent_group_id: DatabaseNodeId::Int(7),
            }),
        },
        json!({ "requestId": "add-group-1", "op": "addGroup", "args": { "parentGroupId": 7 } }),
    );
    assert_roundtrip(
        OperationResponse {
            request_id: "add-group-1".into(),
            outcome: OperationOutcome::Success {
                success: OperationSuccess::AddGroup(AddGroupResult {
                    id: DatabaseNodeId::Int(9),
                }),
            },
        },
        json!({ "requestId": "add-group-1", "status": "success", "op": "addGroup", "result": { "id": 9 } }),
    );
    assert_roundtrip(
        OperationRequest {
            request_id: "delete-group-1".into(),
            operation: Operation::DeleteGroup(DeleteGroupArgs {
                group_id: DatabaseNodeId::Int(9),
            }),
        },
        json!({ "requestId": "delete-group-1", "op": "deleteGroup", "args": { "groupId": 9 } }),
    );
    assert_roundtrip(
        OperationResponse {
            request_id: "delete-group-1".into(),
            outcome: OperationOutcome::Success {
                success: OperationSuccess::DeleteGroup(EmptyResult {}),
            },
        },
        json!({ "requestId": "delete-group-1", "status": "success", "op": "deleteGroup", "result": {} }),
    );
    assert_roundtrip(
        OperationRequest {
            request_id: "rename-1".into(),
            operation: Operation::RenameGroup(RenameGroupArgs {
                group_id: DatabaseNodeId::Int(9),
                name: "Personal".into(),
            }),
        },
        json!({ "requestId": "rename-1", "op": "renameGroup", "args": { "groupId": 9, "name": "Personal" } }),
    );
    assert_roundtrip(
        OperationRequest {
            request_id: "reveal-1".into(),
            operation: Operation::RevealEntryFields(RevealEntryFieldsArgs {
                entry_id: DatabaseNodeId::Int(8),
                field_ids: vec!["standard:Password".into(), "custom:OTP".into()],
                password: None,
            }),
        },
        json!({ "requestId": "reveal-1", "op": "revealEntryFields", "args": { "entryId": 8, "fieldIds": ["standard:Password", "custom:OTP"], "password": null } }),
    );
    assert_roundtrip(
        OperationResponse {
            request_id: "reveal-1".into(),
            outcome: OperationOutcome::Success {
                success: OperationSuccess::RevealEntryFields(RevealEntryFieldsResult {
                    values: vec!["secret".into(), "123456".into()],
                }),
            },
        },
        json!({ "requestId": "reveal-1", "status": "success", "op": "revealEntryFields", "result": { "values": ["secret", "123456"] } }),
    );

    let omitted: RevealEntryFieldsArgs = serde_json::from_value(json!({
        "entryId": 8,
        "fieldIds": ["standard:Password"]
    }))
    .unwrap();
    assert_eq!(omitted.password, None);
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
                    sync_status: SyncStatus::Idle,
                    dirty: false,
                    sync_error: None,
                }),
            },
        },
        json!({
            "requestId": "request-3",
            "status": "success",
            "op": "getDatabaseStatus",
            "result": {
                "status": "not_exist",
                "syncStatus": "idle",
                "dirty": false,
                "syncError": null
            }
        }),
    );
    assert_roundtrip(
        OperationResponse {
            request_id: "search-2".into(),
            outcome: OperationOutcome::Success {
                success: OperationSuccess::SearchEntries(EntriesResult { entries: vec![] }),
            },
        },
        json!({
            "requestId": "search-2",
            "status": "success",
            "op": "searchEntries",
            "result": { "entries": [] }
        }),
    );
}

#[test]
fn merge_response_preserves_sync_error() {
    assert_roundtrip(
        OperationResponse {
            request_id: "merge-1".into(),
            outcome: OperationOutcome::Success {
                success: OperationSuccess::MergeTransferredDatabase(
                    MergeTransferredDatabaseResult {
                        entries_added: 1,
                        entries_modified: 2,
                        entries_deleted: 3,
                        groups_added: 4,
                        groups_modified: 5,
                        groups_deleted: 6,
                        conflict_count: 7,
                        sync_error: Some(OperationError {
                            code: "storage_error".into(),
                            message: "Storage operation failed".into(),
                        }),
                    },
                ),
            },
        },
        json!({
            "requestId": "merge-1",
            "status": "success",
            "op": "mergeTransferredDatabase",
            "result": {
                "entriesAdded": 1,
                "entriesModified": 2,
                "entriesDeleted": 3,
                "groupsAdded": 4,
                "groupsModified": 5,
                "groupsDeleted": 6,
                "conflictCount": 7,
                "syncError": {
                    "code": "storage_error",
                    "message": "Storage operation failed"
                }
            }
        }),
    );
}

#[test]
fn required_nullable_and_empty_types_reject_ambiguous_shapes() {
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
fn all_request_and_success_variants_have_explicit_objects() {
    let requests = [
        json!({ "requestId": "1", "op": "open", "args": { "storage": { "provider": "file", "path": "/vault.kdbx" } } }),
        json!({ "requestId": "2", "op": "lock", "args": { "password": null } }),
        json!({ "requestId": "3", "op": "getConfig", "args": {} }),
        json!({ "requestId": "4", "op": "setConfig", "args": { "config": { "paranoiaMode": true } } }),
        json!({ "requestId": "5", "op": "getEntries", "args": { "excludeTrash": true } }),
        json!({ "requestId": "5a", "op": "searchEntries", "args": { "query": "mail" } }),
        json!({ "requestId": "6", "op": "getGroupHierarchy", "args": {} }),
        json!({ "requestId": "7", "op": "getTags", "args": {} }),
        json!({ "requestId": "8", "op": "getCustomIcons", "args": {} }),
    ];
    for value in requests {
        let parsed: OperationRequest = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }

    for operation in [
        "open",
        "create",
        "unlock",
        "lock",
        "setConfig",
        "moveGroup",
        "moveEntry",
        "renameGroup",
    ] {
        let value = json!({ "requestId": "5", "status": "success", "op": operation, "result": {} });
        let parsed: OperationResponse = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }
}
