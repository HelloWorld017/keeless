use keeless_kdbx::{CompositeKey, DatabaseError, EntryFieldUpdate as KdbxFieldUpdate};
use keeless_schema::{EmptyResult, OperationSuccess, UpdateEntryArgs};
use zeroize::{Zeroize, Zeroizing};

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(
    core: &mut KeelessCore,
    entry_id: keeless_schema::DatabaseNodeId,
    fields: Vec<keeless_schema::EntryFieldUpdate>,
    password: Option<&[u8]>,
) -> Result<EmptyResult> {
    core.enforce_auto_lock();
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let entry_id = parse_node_id(entry_id)?;
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    if handle.database().get_entry(&entry_id).is_none() {
        return Err(CoreError::EntryNotFound);
    }
    let key = if let Some(password) = password {
        let key = CompositeKey::new().with_password(password)?;
        handle.verify_credentials(&key)?;
        key
    } else {
        core.credential
            .as_ref()
            .ok_or(CoreError::PasswordRequired)?
            .restore_key()?
    };
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
    let changed = core
        .handle
        .as_mut()
        .ok_or(CoreError::DatabaseLocked)?
        .apply_update(|database| database.update_entry_fields(&key, &entry_id, &converted))
        .map_err(|error| match error {
            DatabaseError::InvalidFormat(_) => CoreError::InvalidEntryUpdate,
            error => CoreError::from(error),
        })?;
    if changed {
        core.touch_activity();
    }
    Ok(EmptyResult {})
}

pub(super) fn execute(
    core: &mut KeelessCore,
    mut args: UpdateEntryArgs,
) -> Result<OperationSuccess> {
    let password = args
        .password
        .take()
        .map(|password| Zeroizing::new(password.into_bytes()));
    Ok(OperationSuccess::UpdateEntry(run(
        core,
        args.entry_id,
        args.fields,
        password.as_ref().map(|password| password.as_slice()),
    )?))
}
