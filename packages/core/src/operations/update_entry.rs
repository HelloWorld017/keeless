use keeless_kdbx::{
    CompositeKey, DatabaseError, EntryFieldUpdate as KdbxFieldUpdate,
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
    core.enforce_auto_lock();
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
    let changed = core
        .handle
        .as_mut()
        .ok_or(CoreError::DatabaseLocked)?
        .apply_update(|database| {
            database.update_entry(&key, &entry_id, &converted, properties.as_ref())
        })
        .map_err(|error| match error {
            DatabaseError::InvalidFormat(_) => CoreError::InvalidEntryUpdate,
            error => CoreError::from(error),
        })?;
    if changed {
        core.touch_activity();
    }
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
