use keeless_kdbx::NodeId;
use keeless_schema::{AddEntryFromTemplateArgs, AddEntryResult, OperationSuccess};

use crate::model::{node_id, parse_node_id};
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(
    core: &mut KeelessCore,
    args: AddEntryFromTemplateArgs,
) -> Result<AddEntryResult> {
    core.enforce_auto_lock();
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let parent_group_id = parse_node_id(args.parent_group_id)?;
    let template_entry_id = parse_node_id(args.template_entry_id)?;
    let key = core
        .credential
        .as_ref()
        .map(|credential| credential.restore_key())
        .transpose()?;
    let handle = core.handle.as_mut().ok_or(CoreError::DatabaseLocked)?;
    let database = handle.database();
    if database.get_group(&parent_group_id).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    let is_direct_template = database
        .entry_templates_uuid
        .map(NodeId::from_uuid)
        .and_then(|group_id| database.get_group(&group_id))
        .is_some_and(|group| group.child_entry_ids.contains(&template_entry_id));
    if !is_direct_template {
        return Err(CoreError::EntryNotFound);
    }

    let id = handle
        .database_mut()
        .duplicate_entry(&template_entry_id, &parent_group_id, key.as_ref())?
        .ok_or(CoreError::EntryNotFound)?;
    core.touch_activity();
    Ok(AddEntryResult { id: node_id(id) })
}

pub(super) fn execute(
    core: &mut KeelessCore,
    args: AddEntryFromTemplateArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::AddEntryFromTemplate(run(core, args)?))
}
