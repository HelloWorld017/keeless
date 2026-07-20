use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageFrame {
    pub version: u8,
    pub timestamp: i64,
    pub nonce: String,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub ephemeral_public_key: Option<String>,
    pub public_key: String,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub payload: Option<String>,
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageDescriptor {
    pub provider: String,
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseStatus {
    NotExist,
    Locked,
    Unlocked,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeelessConfig {
    #[serde(deserialize_with = "deserialize_nullable")]
    pub auto_lock_timeout_ms: Option<u64>,
    pub paranoia_mode: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeelessConfigPatch {
    #[serde(
        default,
        deserialize_with = "deserialize_present_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub auto_lock_timeout_ms: Option<Option<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(type = bool)]
    pub paranoia_mode: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(untagged)]
pub enum DatabaseNodeId {
    Uuid(String),
    Int(i32),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IconReference {
    pub standard_id: u32,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub custom_uuid: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntrySummary {
    pub id: DatabaseNodeId,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub name: Option<String>,
    pub name_is_protected: bool,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub username: Option<String>,
    pub username_is_protected: bool,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub url: Option<String>,
    pub url_is_protected: bool,
    pub icon: IconReference,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomIcon {
    pub uuid: String,
    pub data_base64: String,
    pub name: String,
    pub last_modification_time_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupHierarchyItem {
    pub id: DatabaseNodeId,
    pub name: String,
    pub icon: IconReference,
    pub child_group_ids: Vec<DatabaseNodeId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TagSummary {
    pub name: String,
    pub entry_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryFieldInformation {
    pub field_index: u64,
    pub name: String,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub value: Option<String>,
    pub is_protected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryAttachmentInformation {
    pub name: String,
    pub size: u64,
    pub is_protected: bool,
}

fn deserialize_present_nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer).map(Some)
}

fn deserialize_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenArgs {
    pub storage: StorageDescriptor,
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnlockArgs {
    pub password: String,
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateArgs {
    pub password: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct LockArgs {}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct GetDatabaseStatusArgs {}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct GetStorageDescriptorArgs {}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct GetConfigArgs {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetConfigArgs {
    pub config: KeelessConfigPatch,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetEntriesArgs {
    #[serde(default = "default_true")]
    #[specta(optional = true)]
    pub exclude_trash: bool,
}

impl Default for GetEntriesArgs {
    fn default() -> Self {
        Self {
            exclude_trash: true,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct GetGroupHierarchyArgs {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetGroupEntriesArgs {
    pub group_id: DatabaseNodeId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct GetTagEntriesArgs {
    pub tag: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct GetTrashEntriesArgs {}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct GetTagsArgs {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetEntryDetailArgs {
    pub entry_id: DatabaseNodeId,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct GetCustomIconsArgs {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MoveGroupArgs {
    pub group_id: DatabaseNodeId,
    pub parent_group_id: DatabaseNodeId,
    pub destination_index: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MoveEntryArgs {
    pub entry_id: DatabaseNodeId,
    pub parent_group_id: DatabaseNodeId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddEntryArgs {
    pub parent_group_id: DatabaseNodeId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddGroupArgs {
    pub parent_group_id: DatabaseNodeId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameGroupArgs {
    pub group_id: DatabaseNodeId,
    pub name: String,
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevealEntryFieldArgs {
    pub entry_id: DatabaseNodeId,
    pub field_index: u64,
    #[serde(default, deserialize_with = "deserialize_nullable")]
    #[specta(optional = true)]
    pub password: Option<String>,
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "op", content = "args", rename_all = "camelCase")]
pub enum Operation {
    Open(OpenArgs),
    Create(CreateArgs),
    Unlock(UnlockArgs),
    Lock(LockArgs),
    GetDatabaseStatus(GetDatabaseStatusArgs),
    GetStorageDescriptor(GetStorageDescriptorArgs),
    GetConfig(GetConfigArgs),
    SetConfig(SetConfigArgs),
    GetEntries(GetEntriesArgs),
    GetGroupHierarchy(GetGroupHierarchyArgs),
    GetGroupEntries(GetGroupEntriesArgs),
    GetTagEntries(GetTagEntriesArgs),
    GetTrashEntries(GetTrashEntriesArgs),
    GetTags(GetTagsArgs),
    GetEntryDetail(GetEntryDetailArgs),
    GetCustomIcons(GetCustomIconsArgs),
    MoveGroup(MoveGroupArgs),
    MoveEntry(MoveEntryArgs),
    AddEntry(AddEntryArgs),
    AddGroup(AddGroupArgs),
    RenameGroup(RenameGroupArgs),
    RevealEntryField(RevealEntryFieldArgs),
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperationRequest {
    pub request_id: String,
    #[serde(flatten)]
    pub operation: Operation,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct EmptyResult {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseStatusResult {
    pub status: DatabaseStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageDescriptorResult {
    #[serde(deserialize_with = "deserialize_nullable")]
    pub storage: Option<StorageDescriptor>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConfigResult {
    pub config: KeelessConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EntriesResult {
    pub entries: Vec<EntrySummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupHierarchyResult {
    pub database_name: String,
    pub root_group_id: DatabaseNodeId,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub recycle_bin_id: Option<DatabaseNodeId>,
    pub groups: Vec<GroupHierarchyItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TagsResult {
    pub tags: Vec<TagSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EntryDetailResult {
    pub id: DatabaseNodeId,
    pub icon: IconReference,
    pub tags: Vec<String>,
    pub fields: Vec<EntryFieldInformation>,
    pub background_color: String,
    pub foreground_color: String,
    pub override_url: String,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub creation_time_ms: Option<i64>,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub last_modification_time_ms: Option<i64>,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub last_access_time_ms: Option<i64>,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub location_changed_ms: Option<i64>,
    pub expires: bool,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub expiry_time_ms: Option<i64>,
    pub usage_count: i64,
    pub attachments: Vec<EntryAttachmentInformation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CustomIconsResult {
    pub icons: Vec<CustomIcon>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AddEntryResult {
    pub id: DatabaseNodeId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AddGroupResult {
    pub id: DatabaseNodeId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RevealEntryFieldResult {
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "op", content = "result", rename_all = "camelCase")]
pub enum OperationSuccess {
    Open(EmptyResult),
    Create(EmptyResult),
    Unlock(EmptyResult),
    Lock(EmptyResult),
    GetDatabaseStatus(DatabaseStatusResult),
    GetStorageDescriptor(StorageDescriptorResult),
    GetConfig(ConfigResult),
    SetConfig(EmptyResult),
    GetEntries(EntriesResult),
    GetGroupHierarchy(GroupHierarchyResult),
    GetGroupEntries(EntriesResult),
    GetTagEntries(EntriesResult),
    GetTrashEntries(EntriesResult),
    GetTags(TagsResult),
    GetEntryDetail(Box<EntryDetailResult>),
    GetCustomIcons(CustomIconsResult),
    MoveGroup(EmptyResult),
    MoveEntry(EmptyResult),
    AddEntry(AddEntryResult),
    AddGroup(AddGroupResult),
    RenameGroup(EmptyResult),
    RevealEntryField(RevealEntryFieldResult),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperationError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum OperationOutcome {
    Success {
        #[serde(flatten)]
        success: OperationSuccess,
    },
    Error {
        error: OperationError,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperationResponse {
    pub request_id: String,
    #[serde(flatten)]
    pub outcome: OperationOutcome,
}
