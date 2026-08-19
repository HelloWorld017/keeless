use keeless_kdbx::{
    CompositeKey, Database, DatabaseError, DateInstant, EntryBinary, EntryFieldId,
    EntryFieldUpdate, EntryPropertiesUpdate as KdbxPropertiesUpdate, EntryUpdate, IconUpdate,
    NodeId, PreparedEntryUpdate,
};
use keeless_schema::{
    EmptyResult, EntryAttachmentUpdate, EntryPropertiesUpdate, OperationSuccess, UpdateEntryArgs,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroize;

use super::{Mutation as JournalMutation, mutate};
use crate::model::{parse_icon_reference, parse_node_id};
use crate::{
    CoreError, KeelessCore, Result, extensions::password_session::PasswordSessionExtension,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Mutation {
    pub(super) id: NodeId,
    pub(super) fields: Vec<JournalEntryField>,
    pub(super) properties: Option<JournalEntryProperties>,
    pub(super) attachments: Vec<JournalEntryAttachment>,
    pub(super) removed_attachment_indices: Vec<u64>,
    pub(super) timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct JournalEntryField {
    pub(super) field_id: Option<String>,
    pub(super) name: String,
    pub(super) value: Option<String>,
    pub(super) is_protected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct JournalEntryProperties {
    pub(super) override_url: String,
    pub(super) tags: Vec<String>,
    pub(super) expires: bool,
    pub(super) expiry_time_ms: Option<i64>,
    pub(super) standard_icon: Option<u32>,
    pub(super) custom_icon: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct JournalEntryAttachment {
    pub(super) name: String,
    pub(super) data: Vec<u8>,
}

impl Drop for JournalEntryField {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

impl Drop for JournalEntryProperties {
    fn drop(&mut self) {
        self.override_url.zeroize();
        self.tags.zeroize();
    }
}

impl Drop for JournalEntryAttachment {
    fn drop(&mut self) {
        self.name.zeroize();
        self.data.zeroize();
    }
}

impl JournalEntryField {
    fn to_kdbx(&self) -> Result<EntryFieldUpdate> {
        Ok(EntryFieldUpdate {
            field_id: self
                .field_id
                .as_deref()
                .map(str::parse)
                .transpose()
                .map_err(|_| CoreError::InvalidJournal)?,
            name: self.name.clone(),
            value: self.value.clone(),
            is_protected: self.is_protected,
        })
    }
}

impl JournalEntryProperties {
    fn to_kdbx(&self) -> KdbxPropertiesUpdate {
        KdbxPropertiesUpdate {
            override_url: self.override_url.clone(),
            tags: self.tags.clone(),
            expires: self.expires,
            expiry_time_ms: self.expiry_time_ms,
            icon: self.standard_icon.map(|standard_id| IconUpdate {
                standard_id,
                custom_uuid: self.custom_icon,
            }),
        }
    }
}

pub(super) fn prepare(
    database: &Database,
    mutation: &Mutation,
    key: &CompositeKey,
) -> Result<Option<PreparedEntryUpdate>> {
    let mut fields = mutation
        .fields
        .iter()
        .map(JournalEntryField::to_kdbx)
        .collect::<Result<Vec<_>>>()?;
    for field in &mut fields {
        if matches!(
            field.field_id,
            Some(EntryFieldId::Standard(
                keeless_kdbx::StandardField::Password
            ))
        ) && field.value.is_none()
            && let Some(source) = database
                .get_entry(&mutation.id)
                .and_then(|entry| entry.field(field.field_id.expect("password field ID")))
            && !source.value().is_protected()
        {
            field.value = Some(source.value().as_str().to_string());
        }
    }
    let properties = mutation
        .properties
        .as_ref()
        .map(JournalEntryProperties::to_kdbx);
    let attachments = mutation
        .attachments
        .iter()
        .map(|attachment| EntryBinary {
            name: attachment.name.clone(),
            data: attachment.data.clone(),
            is_protected: false,
        })
        .collect::<Vec<_>>();
    let update = EntryUpdate {
        fields,
        properties,
        attachments,
        removed_attachment_indices: mutation.removed_attachment_indices.clone(),
        last_modification_time: DateInstant::EpochMillis(mutation.timestamp_ms),
    };
    database
        .prepare_entry_update(key, &mutation.id, &update)
        .map_err(CoreError::from)
}

pub(super) fn apply(
    database: &mut Database,
    mutation: &Mutation,
    key: &CompositeKey,
) -> Result<()> {
    let prepared = prepare(database, mutation, key)?.ok_or(CoreError::InvalidJournal)?;
    database.commit_entry_update(prepared);
    Ok(())
}

pub(crate) async fn run(
    core: &mut KeelessCore,
    entry_id: keeless_schema::DatabaseNodeId,
    fields: Vec<keeless_schema::EntryFieldUpdate>,
    properties: Option<EntryPropertiesUpdate>,
    attachment_updates: Vec<EntryAttachmentUpdate>,
    removed_attachment_indices: Vec<u64>,
    password: Option<&[u8]>,
) -> Result<EmptyResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let entry_id = parse_node_id(entry_id)?;
    if core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database()
        .get_entry(&entry_id)
        .is_none()
    {
        return Err(CoreError::EntryNotFound);
    }
    let field_count = fields.len();
    let mut fields = fields.into_iter();
    let mut converted = Vec::with_capacity(field_count);
    while let Some(mut field) = fields.next() {
        let field_id = match field.field_id.as_deref().map(str::parse).transpose() {
            Ok(id) => id,
            Err(()) => {
                field.value.zeroize();
                for mut field in fields {
                    field.value.zeroize();
                }
                return Err(CoreError::InvalidEntryUpdate);
            }
        };
        converted.push(EntryFieldUpdate {
            field_id,
            name: field.name,
            value: field.value,
            is_protected: field.is_protected,
        });
    }
    let properties = properties
        .map(|properties| {
            let icon = properties
                .icon
                .as_ref()
                .map(|icon| {
                    parse_icon_reference(
                        core.handle
                            .as_ref()
                            .ok_or(CoreError::DatabaseLocked)?
                            .database(),
                        icon,
                    )
                })
                .transpose()?;
            Ok::<_, CoreError>(KdbxPropertiesUpdate {
                override_url: properties.override_url,
                tags: properties.tags,
                expires: properties.expires,
                expiry_time_ms: properties.expiry_time_ms,
                icon,
            })
        })
        .transpose()?;
    let key = if let Some(password) = password {
        let key = CompositeKey::new().with_password(password)?;
        core.handle
            .as_ref()
            .ok_or(CoreError::DatabaseLocked)?
            .verify_credentials(&key)?;
        key
    } else if let Some(credential) = &core.credential {
        credential.restore_key()?
    } else {
        let password = core
            .request_password(crate::PasswordInputMode::Save)
            .await?;
        let key = CompositeKey::new().with_password(&password)?;
        core.handle
            .as_ref()
            .ok_or(CoreError::DatabaseLocked)?
            .verify_credentials(&key)?;
        key
    };
    let timestamp_ms = core.clock.now_millis();
    let mut attachments = Vec::with_capacity(attachment_updates.len());
    for attachment in attachment_updates {
        let mut data = core.consume_upload_transfer(&attachment.transfer_id)?;
        attachments.push(EntryBinary {
            name: attachment.name,
            data: std::mem::take(&mut *data),
            is_protected: false,
        });
    }
    let update = EntryUpdate {
        fields: converted,
        properties,
        attachments,
        removed_attachment_indices,
        last_modification_time: DateInstant::EpochMillis(timestamp_ms),
    };
    let prepared = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database()
        .prepare_entry_update(&key, &entry_id, &update)
        .map_err(|error| match error {
            DatabaseError::InvalidFormat(_) => CoreError::InvalidEntryUpdate,
            error => CoreError::from(error),
        })?;
    let Some(prepared) = prepared else {
        return Ok(EmptyResult {});
    };
    let payload = Mutation {
        id: entry_id,
        fields: update
            .fields
            .iter()
            .map(|field| JournalEntryField {
                field_id: field.field_id.map(|id| id.to_string()),
                name: field.name.clone(),
                value: field.value.clone(),
                is_protected: field.is_protected,
            })
            .collect(),
        properties: update
            .properties
            .as_ref()
            .map(|properties| JournalEntryProperties {
                override_url: properties.override_url.clone(),
                tags: properties.tags.clone(),
                expires: properties.expires,
                expiry_time_ms: properties.expiry_time_ms,
                standard_icon: properties.icon.map(|icon| icon.standard_id),
                custom_icon: properties.icon.and_then(|icon| icon.custom_uuid),
            }),
        attachments: update
            .attachments
            .iter()
            .map(|attachment| JournalEntryAttachment {
                name: attachment.name.clone(),
                data: attachment.data.clone(),
            })
            .collect(),
        removed_attachment_indices: update.removed_attachment_indices.clone(),
        timestamp_ms,
    };
    let mutation = JournalMutation::UpdateEntry(payload);
    mutate(core, &mutation, move |database| {
        database.commit_entry_update(prepared)
    })
    .await?;
    core.touch_activity();
    Ok(EmptyResult {})
}

pub(crate) async fn execute(
    core: &mut KeelessCore,
    mut args: UpdateEntryArgs,
) -> Result<OperationSuccess> {
    let password = {
        let now_millis = core.clock.monotonic_millis();
        core.extensions
            .get_mut::<PasswordSessionExtension>()
            .resolve_argument(
                args.password.take(),
                args.password_session.take(),
                now_millis,
            )?
    };
    Ok(OperationSuccess::UpdateEntry(
        run(
            core,
            args.entry_id,
            args.fields,
            args.properties,
            args.attachments.unwrap_or_default(),
            args.removed_attachment_indices.unwrap_or_default(),
            password.as_ref().map(|password| password.as_slice()),
        )
        .await?,
    ))
}
