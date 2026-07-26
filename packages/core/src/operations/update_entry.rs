use keeless_kdbx::{
    CompositeKey, DatabaseError, DateInstant, EntryFieldUpdate as KdbxFieldUpdate,
    EntryPropertiesUpdate as KdbxPropertiesUpdate,
};
use keeless_schema::{EmptyResult, EntryPropertiesUpdate, OperationSuccess, UpdateEntryArgs};
use zeroize::{Zeroize, Zeroizing};

use crate::model::{parse_icon_reference, parse_node_id};
use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn run(
    core: &mut KeelessCore,
    entry_id: keeless_schema::DatabaseNodeId,
    fields: Vec<keeless_schema::EntryFieldUpdate>,
    properties: Option<EntryPropertiesUpdate>,
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
        converted.push(KdbxFieldUpdate {
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
    let new_custom_field_ids = converted
        .iter()
        .filter(|field| field.field_id.is_none())
        .map(|_| uuid::Uuid::new_v4())
        .collect::<Vec<_>>();
    let timestamp_ms = core.clock.now_millis();
    let prepared = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database()
        .prepare_entry_update(
            &key,
            &entry_id,
            &converted,
            properties.as_ref(),
            &new_custom_field_ids,
            DateInstant::EpochMillis(timestamp_ms),
        )
        .map_err(|error| match error {
            DatabaseError::InvalidFormat(_) => CoreError::InvalidEntryUpdate,
            error => CoreError::from(error),
        })?;
    let Some(prepared) = prepared else {
        return Ok(EmptyResult {});
    };
    let journal_fields = converted
        .iter()
        .map(|field| super::mutations::JournalEntryField {
            field_id: field.field_id.map(|id| id.to_string()),
            name: field.name.clone(),
            value: field.value.clone(),
            is_protected: field.is_protected,
        })
        .collect();
    let journal_properties =
        properties
            .as_ref()
            .map(|properties| super::mutations::JournalEntryProperties {
                override_url: properties.override_url.clone(),
                tags: properties.tags.clone(),
                expires: properties.expires,
                expiry_time_ms: properties.expiry_time_ms,
                standard_icon: properties.icon.map(|icon| icon.standard_id),
                custom_icon: properties.icon.and_then(|icon| icon.custom_uuid),
            });
    let mutation = super::mutations::Mutation::UpdateEntry {
        id: entry_id,
        fields: journal_fields,
        properties: journal_properties,
        new_custom_field_ids,
        timestamp_ms,
    };
    super::mutations::mutate(core, &mutation, move |database| {
        database.commit_entry_update(prepared);
    })
    .await?;
    core.touch_activity();
    Ok(EmptyResult {})
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: UpdateEntryArgs,
) -> Result<OperationSuccess> {
    let password = args
        .password
        .take()
        .map(|password| Zeroizing::new(password.into_bytes()));
    Ok(OperationSuccess::UpdateEntry(
        run(
            core,
            args.entry_id,
            args.fields,
            args.properties,
            password.as_ref().map(|password| password.as_slice()),
        )
        .await?,
    ))
}
