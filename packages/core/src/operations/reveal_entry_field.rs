use keeless_kdbx::{CompositeKey, EntryFieldId};
use keeless_schema::{OperationSuccess, RevealEntryFieldArgs, RevealEntryFieldResult};
use zeroize::Zeroizing;

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn run(
    core: &mut KeelessCore,
    entry_id: keeless_schema::DatabaseNodeId,
    field_id: String,
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
    let field_id = field_id
        .parse::<EntryFieldId>()
        .map_err(|_| CoreError::InvalidEntryField)?;
    let field = entry.field(field_id).ok_or(CoreError::InvalidEntryField)?;
    if !field.value().is_protected() {
        return Err(CoreError::InvalidEntryField);
    }
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
            .request_password(crate::PasswordInputMode::Reveal)
            .await?;
        let key = CompositeKey::new().with_password(&password)?;
        core.handle
            .as_ref()
            .ok_or(CoreError::DatabaseLocked)?
            .verify_credentials(&key)?;
        key
    };
    let value = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database()
        .with_entry_field_id(&key, &entry_id, field_id, str::to_owned)?;
    core.touch_activity();
    Ok(RevealEntryFieldResult { value })
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: RevealEntryFieldArgs,
) -> Result<OperationSuccess> {
    let password = args
        .password
        .take()
        .map(|password| Zeroizing::new(password.into_bytes()));
    Ok(OperationSuccess::RevealEntryField(
        run(
            core,
            args.entry_id,
            args.field_id,
            password.as_ref().map(|password| password.as_slice()),
        )
        .await?,
    ))
}
