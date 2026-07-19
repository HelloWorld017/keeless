use keeless_schema::{EmptyResult, MoveEntryArgs, OperationSuccess};

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: MoveEntryArgs) -> Result<EmptyResult> {
    core.enforce_auto_lock();
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }

    let entry_id = parse_node_id(args.entry_id)?;
    let parent_group_id = parse_node_id(args.parent_group_id)?;
    let handle = core.handle.as_mut().ok_or(CoreError::DatabaseLocked)?;
    if handle.database().get_entry(&entry_id).is_none() {
        return Err(CoreError::EntryNotFound);
    }
    if handle.database().get_group(&parent_group_id).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    if !handle
        .database_mut()
        .reposition_entry(&entry_id, &parent_group_id)
    {
        return Err(CoreError::InvalidEntryMove);
    }

    core.touch_activity();
    Ok(EmptyResult {})
}

pub(super) fn execute(core: &mut KeelessCore, args: MoveEntryArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::MoveEntry(run(core, args)?))
}
