use keeless_kdbx::{CompositeKey, EntryFieldSelector};
use keeless_schema::{OperationSuccess, RevealEntryFieldArgs, RevealEntryFieldResult};
use zeroize::Zeroizing;

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(
    core: &mut KeelessCore,
    entry_id: keeless_schema::DatabaseNodeId,
    field_index: u64,
    password: Option<&[u8]>,
) -> Result<RevealEntryFieldResult> {
    core.enforce_auto_lock();
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let entry_id = parse_node_id(entry_id)?;
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    let entry = handle
        .database()
        .get_entry(&entry_id)
        .ok_or(CoreError::EntryNotFound)?;
    let (selector, custom_index) = match field_index {
        0 if entry.title_is_protected => (Some(EntryFieldSelector::Title), None),
        1 if entry.username.is_protected() => (Some(EntryFieldSelector::UserName), None),
        2 if entry.password.is_protected() => (Some(EntryFieldSelector::Password), None),
        3 if entry.url_is_protected => (Some(EntryFieldSelector::Url), None),
        4 if entry.notes.is_protected() => (Some(EntryFieldSelector::Notes), None),
        index if index >= 5 => {
            let index: usize = (index - 5)
                .try_into()
                .map_err(|_| CoreError::InvalidEntryField)?;
            let field = entry
                .custom_fields
                .get(index)
                .ok_or(CoreError::InvalidEntryField)?;
            if !field.is_protected && !field.value.is_protected() {
                return Err(CoreError::InvalidEntryField);
            }
            (None, Some(index))
        }
        _ => return Err(CoreError::InvalidEntryField),
    };

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
    let value = if let Some(index) = custom_index {
        handle
            .database()
            .with_entry_custom_field(&key, &entry_id, index, str::to_owned)?
    } else {
        handle.database().with_entry_field(
            &key,
            &entry_id,
            selector.as_ref().expect("standard field selector"),
            str::to_owned,
        )?
    };
    core.touch_activity();
    Ok(RevealEntryFieldResult { value })
}

pub(super) fn execute(
    core: &mut KeelessCore,
    mut args: RevealEntryFieldArgs,
) -> Result<OperationSuccess> {
    let password = args
        .password
        .take()
        .map(|password| Zeroizing::new(password.into_bytes()));
    Ok(OperationSuccess::RevealEntryField(run(
        core,
        args.entry_id,
        args.field_index,
        password.as_ref().map(|password| password.as_slice()),
    )?))
}
