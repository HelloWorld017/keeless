use keeless_kdbx::{CompositeKey, EntryFieldId};
use keeless_schema::{OperationSuccess, RevealEntryFieldsArgs, RevealEntryFieldsResult};
use zeroize::Zeroizing;

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn run(
    core: &mut KeelessCore,
    entry_id: keeless_schema::DatabaseNodeId,
    field_ids: Vec<String>,
    password: Option<&[u8]>,
) -> Result<RevealEntryFieldsResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let entry_id = parse_node_id(entry_id)?;
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    let entry = handle
        .database()
        .get_entry(&entry_id)
        .ok_or(CoreError::EntryNotFound)?;
    let field_ids = field_ids
        .into_iter()
        .map(|field_id| {
            let field_id = field_id
                .parse::<EntryFieldId>()
                .map_err(|_| CoreError::InvalidEntryField)?;
            let field = entry.field(field_id).ok_or(CoreError::InvalidEntryField)?;
            if !field.value().is_protected() {
                return Err(CoreError::InvalidEntryField);
            }
            Ok(field_id)
        })
        .collect::<Result<Vec<_>>>()?;
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
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    let values = field_ids
        .into_iter()
        .map(|field_id| {
            handle
                .database()
                .with_entry_field_id(&key, &entry_id, field_id, str::to_owned)
                .map_err(CoreError::from)
        })
        .collect::<Result<Vec<_>>>()?;
    core.touch_activity();
    Ok(RevealEntryFieldsResult { values })
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: RevealEntryFieldsArgs,
) -> Result<OperationSuccess> {
    let password = args
        .password
        .take()
        .map(|password| Zeroizing::new(password.into_bytes()));
    Ok(OperationSuccess::RevealEntryFields(
        run(
            core,
            args.entry_id,
            args.field_ids,
            password.as_ref().map(|password| password.as_slice()),
        )
        .await?,
    ))
}
