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
pub struct GetConfigArgs {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetConfigArgs {
    pub config: KeelessConfigPatch,
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "op", content = "args", rename_all = "camelCase")]
pub enum Operation {
    Open(OpenArgs),
    Create(CreateArgs),
    Unlock(UnlockArgs),
    Lock(LockArgs),
    GetDatabaseStatus(GetDatabaseStatusArgs),
    GetConfig(GetConfigArgs),
    SetConfig(SetConfigArgs),
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
pub struct ConfigResult {
    pub config: KeelessConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "op", content = "result", rename_all = "camelCase")]
pub enum OperationSuccess {
    Open(EmptyResult),
    Create(EmptyResult),
    Unlock(EmptyResult),
    Lock(EmptyResult),
    GetDatabaseStatus(DatabaseStatusResult),
    GetConfig(ConfigResult),
    SetConfig(EmptyResult),
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
