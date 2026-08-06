mod operation_schema;

use operation_schema::operation_schema;
use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Idle,
    Syncing,
    Error,
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
pub struct TagStyle {
    pub icon: IconReference,
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TagSummary {
    pub name: String,
    pub entry_count: u64,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub style: Option<TagStyle>,
    pub can_delete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EntryFieldKind {
    Title,
    UserName,
    Password,
    Url,
    Notes,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EntryFieldInformation {
    Field {
        order: u64,
        #[serde(rename = "fieldId", deserialize_with = "deserialize_nullable")]
        field_id: Option<String>,
        kind: EntryFieldKind,
        name: String,
        label: String,
        #[serde(deserialize_with = "deserialize_nullable")]
        value: Option<String>,
        #[serde(rename = "isProtected")]
        is_protected: bool,
        #[serde(rename = "isInternal")]
        is_internal: bool,
        #[serde(deserialize_with = "deserialize_nullable")]
        control: Option<FieldControl>,
    },
    TimeOtp {
        order: u64,
        label: String,
    },
    PasswordConfirmation {
        order: u64,
        label: String,
        #[serde(rename = "passwordFieldId")]
        password_field_id: String,
        control: FieldControl,
    },
    OverrideUrl {
        order: u64,
        label: String,
        control: FieldControl,
    },
    Expiry {
        order: u64,
        label: String,
        control: FieldControl,
    },
    Tags {
        order: u64,
        label: String,
        control: FieldControl,
    },
    Divider {
        order: u64,
        label: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryAttachmentInformation {
    pub index: u64,
    pub name: String,
    pub size: u64,
    pub is_protected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FieldControl {
    Text { protected: bool, lines: u8 },
    Url,
    Popout { protected: bool },
    RichText { lines: u8 },
    Date,
    Time,
    DateTime,
    Checkbox,
    Select { options: Vec<String> },
    Divider,
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

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryFieldUpdate {
    #[serde(deserialize_with = "deserialize_nullable")]
    pub field_id: Option<String>,
    pub name: String,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub value: Option<String>,
    pub is_protected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryPropertiesUpdate {
    pub override_url: String,
    pub tags: Vec<String>,
    pub expires: bool,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub expiry_time_ms: Option<i64>,
    #[serde(
        default,
        deserialize_with = "deserialize_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[specta(optional = true)]
    pub icon: Option<IconReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryAttachmentUpdate {
    pub transfer_id: String,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationResource {
    DatabaseStatus,
    StorageDescriptor,
    Config,
    Entry,
    Group,
    Tag,
    CustomIcon,
}

impl OperationResource {
    pub const fn name(self) -> &'static str {
        match self {
            Self::DatabaseStatus => "databaseStatus",
            Self::StorageDescriptor => "storageDescriptor",
            Self::Config => "config",
            Self::Entry => "entry",
            Self::Group => "group",
            Self::Tag => "tag",
            Self::CustomIcon => "customIcon",
        }
    }
}

pub const OPERATION_RESOURCES: &[OperationResource] = &[
    OperationResource::DatabaseStatus,
    OperationResource::StorageDescriptor,
    OperationResource::Config,
    OperationResource::Entry,
    OperationResource::Group,
    OperationResource::Tag,
    OperationResource::CustomIcon,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperationMetadata {
    pub queries: &'static [OperationResource],
    pub fetches: &'static [OperationResource],
    pub mutates: &'static [OperationResource],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperationDefinition {
    pub name: &'static str,
    pub metadata: OperationMetadata,
}

operation_schema! {
    Open("open") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct OpenArgs {
                pub storage: StorageDescriptor,
            }
        }
        result {
            #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(deny_unknown_fields)]
            pub struct EmptyResult {}
        }
        mutates: [DatabaseStatus],
    }

    Create("create") {
        args {
            #[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct CreateArgs {
                #[serde(default, deserialize_with = "deserialize_nullable")]
                #[specta(optional = true)]
                pub password: Option<String>,
            }
        }
        result EmptyResult
        mutates: [DatabaseStatus],
    }

    Unlock("unlock") {
        args {
            #[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct UnlockArgs {
                #[serde(default, deserialize_with = "deserialize_nullable")]
                #[specta(optional = true)]
                pub password: Option<String>,
            }
        }
        result EmptyResult
        mutates: [DatabaseStatus],
    }

    Lock("lock") {
        args {
            #[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct LockArgs {
                #[serde(default, deserialize_with = "deserialize_nullable")]
                #[specta(optional = true)]
                pub password: Option<String>,
            }
        }
        result EmptyResult
        mutates: [DatabaseStatus],
    }

    GetDatabaseStatus("getDatabaseStatus") {
        args {
            #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(deny_unknown_fields)]
            pub struct GetDatabaseStatusArgs {}
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct DatabaseStatusResult {
                pub status: DatabaseStatus,
                pub sync_status: SyncStatus,
                pub dirty: bool,
                #[serde(deserialize_with = "deserialize_nullable")]
                pub sync_error: Option<OperationError>,
            }
        }
        queries: [DatabaseStatus],
    }

    GetStorageDescriptor("getStorageDescriptor") {
        args {
            #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(deny_unknown_fields)]
            pub struct GetStorageDescriptorArgs {}
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct StorageDescriptorResult {
                #[serde(deserialize_with = "deserialize_nullable")]
                pub storage: Option<StorageDescriptor>,
            }
        }
        queries: [StorageDescriptor],
    }

    GetConfig("getConfig") {
        args {
            #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(deny_unknown_fields)]
            pub struct GetConfigArgs {}
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct ConfigResult {
                pub config: KeelessConfig,
            }
        }
        queries: [Config],
    }

    SetConfig("setConfig") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct SetConfigArgs {
                pub config: KeelessConfigPatch,
            }
        }
        result EmptyResult
        mutates: [Config, DatabaseStatus],
    }

    GetEntries("getEntries") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct GetEntriesArgs {
                #[serde(default = "default_true")]
                #[specta(optional = true)]
                pub exclude_trash: bool,
            }
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct EntriesResult {
                pub entries: Vec<EntrySummary>,
            }
        }
        queries: [Entry],
    }

    SearchEntries("searchEntries") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct SearchEntriesArgs {
                pub query: String,
            }
        }
        result EntriesResult
        queries: [Entry],
    }

    SearchFuzzy("searchFuzzy") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct SearchFuzzyArgs {
                pub query: String,
            }
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct SearchFuzzyResult {
                pub entries: Vec<EntrySummary>,
                pub groups: Vec<GroupHierarchyItem>,
                pub tags: Vec<TagSummary>,
                pub trash_matches: bool,
                pub filter_tokens: Vec<String>,
            }
        }
        queries: [Entry, Group, Tag],
    }

    GetGroupHierarchy("getGroupHierarchy") {
        args {
            #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(deny_unknown_fields)]
            pub struct GetGroupHierarchyArgs {}
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct GroupHierarchyResult {
                pub database_name: String,
                pub root_group_id: DatabaseNodeId,
                #[serde(deserialize_with = "deserialize_nullable")]
                pub recycle_bin_id: Option<DatabaseNodeId>,
                pub groups: Vec<GroupHierarchyItem>,
            }
        }
        queries: [Group],
    }

    GetGroupEntries("getGroupEntries") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct GetGroupEntriesArgs {
                pub group_id: DatabaseNodeId,
            }
        }
        result EntriesResult
        queries: [Entry],
    }

    GetTagEntries("getTagEntries") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(deny_unknown_fields)]
            pub struct GetTagEntriesArgs {
                pub tag: String,
            }
        }
        result EntriesResult
        queries: [Entry, Tag],
    }

    GetTrashEntries("getTrashEntries") {
        args {
            #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(deny_unknown_fields)]
            pub struct GetTrashEntriesArgs {}
        }
        result EntriesResult
        queries: [Entry],
    }

    GetTags("getTags") {
        args {
            #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(deny_unknown_fields)]
            pub struct GetTagsArgs {}
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct TagsResult {
                pub tags: Vec<TagSummary>,
            }
        }
        queries: [Tag],
    }

    GetEntryDetail("getEntryDetail") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct GetEntryDetailArgs {
                pub entry_id: DatabaseNodeId,
            }
        }
        result Box {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct EntryDetailResult {
                pub id: DatabaseNodeId,
                pub is_template: bool,
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
        }
        queries: [Entry],
    }

    UpdateEntry("updateEntry") {
        args {
            #[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct UpdateEntryArgs {
                pub entry_id: DatabaseNodeId,
                pub fields: Vec<EntryFieldUpdate>,
                #[serde(default, deserialize_with = "deserialize_nullable")]
                #[specta(optional = true)]
                pub properties: Option<EntryPropertiesUpdate>,
                #[serde(
                    default,
                    deserialize_with = "deserialize_nullable",
                    skip_serializing_if = "Option::is_none"
                )]
                #[specta(optional = true)]
                pub attachments: Option<Vec<EntryAttachmentUpdate>>,
                #[serde(
                    default,
                    deserialize_with = "deserialize_nullable",
                    skip_serializing_if = "Option::is_none"
                )]
                #[specta(optional = true)]
                pub removed_attachment_indices: Option<Vec<u64>>,
                #[serde(default, deserialize_with = "deserialize_nullable")]
                #[specta(optional = true)]
                pub password: Option<String>,
            }
        }
        result EmptyResult
        mutates: [DatabaseStatus, Entry, Tag],
    }

    PrepareEntryAttachmentDownload("prepareEntryAttachmentDownload") {
        args {
            #[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct PrepareEntryAttachmentDownloadArgs {
                pub entry_id: DatabaseNodeId,
                pub attachment_index: u64,
                pub name: String,
            }
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct PrepareEntryAttachmentDownloadResult {
                pub transfer_id: String,
            }
        }
        fetches: [Entry],
    }

    DeleteEntry("deleteEntry") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct DeleteEntryArgs {
                pub entry_id: DatabaseNodeId,
                pub permanent: bool,
            }
        }
        result EmptyResult
        mutates: [DatabaseStatus, Entry, Tag],
    }

    EmptyRecycleBin("emptyRecycleBin") {
        args {
            #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(deny_unknown_fields)]
            pub struct EmptyRecycleBinArgs {}
        }
        result EmptyResult
        mutates: [DatabaseStatus, Entry, Group, Tag],
    }

    SaveDatabase("saveDatabase") {
        args {
            #[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct SaveDatabaseArgs {
                #[serde(default, deserialize_with = "deserialize_nullable")]
                #[specta(optional = true)]
                pub password: Option<String>,
            }
        }
        result EmptyResult
        mutates: [DatabaseStatus],
    }

    PrepareDatabaseExport("prepareDatabaseExport") {
        args {
            #[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct PrepareDatabaseExportArgs {
                #[serde(default, deserialize_with = "deserialize_nullable")]
                #[specta(optional = true)]
                pub password: Option<String>,
            }
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct PrepareDatabaseExportResult {
                pub transfer_id: String,
            }
        }
        fetches: [Entry, Group, Tag, CustomIcon],
    }

    MergeTransferredDatabase("mergeTransferredDatabase") {
        args {
            #[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct MergeTransferredDatabaseArgs {
                pub transfer_id: String,
                pub source_password: String,
                #[serde(default, deserialize_with = "deserialize_nullable")]
                #[specta(optional = true)]
                pub password: Option<String>,
            }
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct MergeTransferredDatabaseResult {
                pub entries_added: u64,
                pub entries_modified: u64,
                pub entries_deleted: u64,
                pub groups_added: u64,
                pub groups_modified: u64,
                pub groups_deleted: u64,
                pub conflict_count: u64,
                #[serde(deserialize_with = "deserialize_nullable")]
                pub sync_error: Option<OperationError>,
            }
        }
        mutates: [DatabaseStatus, Entry, Group, Tag, CustomIcon],
    }

    GetCustomIcons("getCustomIcons") {
        args {
            #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(deny_unknown_fields)]
            pub struct GetCustomIconsArgs {}
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct CustomIconsResult {
                pub icons: Vec<CustomIcon>,
            }
        }
        queries: [CustomIcon],
    }

    GetEntryTemplates("getEntryTemplates") {
        args {
            #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(deny_unknown_fields)]
            pub struct GetEntryTemplatesArgs {}
        }
        result EntriesResult
        queries: [Entry],
    }

    MoveGroup("moveGroup") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct MoveGroupArgs {
                pub group_id: DatabaseNodeId,
                pub parent_group_id: DatabaseNodeId,
                pub destination_index: u64,
            }
        }
        result EmptyResult
        mutates: [DatabaseStatus, Group],
    }

    MoveEntry("moveEntry") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct MoveEntryArgs {
                pub entry_id: DatabaseNodeId,
                pub parent_group_id: DatabaseNodeId,
            }
        }
        result EmptyResult
        mutates: [DatabaseStatus, Entry, Tag],
    }

    AddEntry("addEntry") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct AddEntryArgs {
                pub parent_group_id: DatabaseNodeId,
            }
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct AddEntryResult {
                pub id: DatabaseNodeId,
            }
        }
        mutates: [DatabaseStatus, Entry, Tag],
    }

    AddEntryFromTemplate("addEntryFromTemplate") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct AddEntryFromTemplateArgs {
                pub parent_group_id: DatabaseNodeId,
                pub template_entry_id: DatabaseNodeId,
            }
        }
        result AddEntryResult
        mutates: [DatabaseStatus, Entry, Tag],
    }

    AddGroup("addGroup") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct AddGroupArgs {
                pub parent_group_id: DatabaseNodeId,
            }
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct AddGroupResult {
                pub id: DatabaseNodeId,
            }
        }
        mutates: [DatabaseStatus, Group],
    }

    DeleteGroup("deleteGroup") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct DeleteGroupArgs {
                pub group_id: DatabaseNodeId,
            }
        }
        result EmptyResult
        mutates: [DatabaseStatus, Entry, Group, Tag],
    }

    RenameGroup("renameGroup") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct RenameGroupArgs {
                pub group_id: DatabaseNodeId,
                pub name: String,
            }
        }
        result EmptyResult
        mutates: [DatabaseStatus, Group],
    }

    UpdateGroup("updateGroup") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct UpdateGroupArgs {
                pub group_id: DatabaseNodeId,
                pub name: String,
                pub icon: IconReference,
            }
        }
        result EmptyResult
        mutates: [DatabaseStatus, Group],
    }

    UpdateTagStyle("updateTagStyle") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct UpdateTagStyleArgs {
                pub name: String,
                pub style: TagStyle,
            }
        }
        result EmptyResult
        mutates: [DatabaseStatus, Tag],
    }

    DeleteTag("deleteTag") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct DeleteTagArgs {
                pub name: String,
            }
        }
        result EmptyResult
        mutates: [DatabaseStatus, Tag],
    }

    RevealEntryFields("revealEntryFields") {
        args {
            #[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct RevealEntryFieldsArgs {
                pub entry_id: DatabaseNodeId,
                pub field_ids: Vec<String>,
                #[serde(default, deserialize_with = "deserialize_nullable")]
                #[specta(optional = true)]
                pub password: Option<String>,
            }
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct RevealEntryFieldsResult {
                pub values: Vec<String>,
            }
        }
        fetches: [Entry],
    }

    GetEntryTotp("getEntryTotp") {
        args {
            #[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct GetEntryTotpArgs {
                pub entry_id: DatabaseNodeId,
                #[serde(default, deserialize_with = "deserialize_nullable")]
                #[specta(optional = true)]
                pub field_id: Option<String>,
                #[serde(default, deserialize_with = "deserialize_nullable")]
                #[specta(optional = true)]
                pub password: Option<String>,
            }
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase")]
            pub struct GetEntryTotpResult {
                pub code: String,
                pub digits: u32,
                pub period: u32,
                pub expires_at_ms: i64,
            }
        }
        fetches: [Entry],
    }

    GetPasskeys("getPasskeys") {
        args {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct GetPasskeysArgs {
                // Limits the result to one relying party when set.
                #[serde(default, deserialize_with = "deserialize_nullable")]
                #[specta(optional = true)]
                pub rp_id: Option<String>,
                // Credential IDs from CTAP2 `allowList`, base64url without padding.
                #[serde(default)]
                pub allow_credential_ids: Vec<String>,
            }
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct PasskeysResult {
                pub credentials: Vec<PasskeySummary>,
            }
        }
        queries: [Entry],
    }

    RegisterPasskey("registerPasskey") {
        args {
            // Registration inputs from a CTAP2 `authenticatorMakeCredential` request.
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct RegisterPasskeyArgs {
                pub rp_id: String,
                // Relying-party display name, used as the entry title. Falls back to `rpId`.
                #[serde(default, deserialize_with = "deserialize_nullable")]
                #[specta(optional = true)]
                pub rp_name: Option<String>,
                pub user_name: String,
                // User handle, base64url without padding.
                pub user_handle: String,
                // SHA-256 of the client data, base64url without padding.
                pub client_data_hash: String,
                // COSE algorithm identifiers in relying-party preference order.
                pub algorithms: Vec<i32>,
                // Credential IDs from the CTAP2 `excludeList`, base64url without padding.
                pub exclude_credential_ids: Vec<String>,
                // Whether the caller completed its user-verification ceremony.
                pub user_verified: bool,
            }
        }

        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct RegisterPasskeyResult {
                pub entry_id: DatabaseNodeId,
                // Credential ID, base64url without padding.
                pub credential_id: String,
                // Authenticator data to place in the attestation object, base64url without padding.
                pub authenticator_data: String,
            }
        }
        mutates: [DatabaseStatus, Entry],
    }

    AssertPasskey("assertPasskey") {
        args {
            // Assertion inputs from a CTAP2 `authenticatorGetAssertion` request.
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct AssertPasskeyArgs {
                // Entry holding the credential, chosen from a prior `getPasskeys`.
                pub entry_id: DatabaseNodeId,
                pub rp_id: String,
                // SHA-256 of the client data, base64url without padding.
                pub client_data_hash: String,
                // Whether the user approved this ceremony. A silent assertion, which a
                // platform uses to discover credentials before prompting, sets this false
                // and produces an assertion relying parties reject.
                pub user_present: bool,
                // Whether the caller completed its user-verification ceremony.
                pub user_verified: bool,
            }
        }
        result {
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct AssertPasskeyResult {
                // Credential ID, base64url without padding.
                pub credential_id: String,
                // Signed authenticator data, base64url without padding.
                pub authenticator_data: String,
                // Assertion signature, base64url without padding.
                pub signature: String,
                // User handle, base64url without padding.
                pub user_handle: String,
            }
        }
        fetches: [Entry],
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperationRequest {
    pub request_id: String,
    #[serde(flatten)]
    pub operation: Operation,
}

/// One stored passkey, as much as can be shown without signing anything.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PasskeySummary {
    pub entry_id: DatabaseNodeId,
    pub rp_id: String,
    pub username: String,
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
