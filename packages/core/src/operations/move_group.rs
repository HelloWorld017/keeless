use keeless_schema::{EmptyResult, MoveGroupArgs, OperationSuccess};

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: MoveGroupArgs) -> Result<EmptyResult> {
    core.enforce_auto_lock();
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }

    let group_id = parse_node_id(args.group_id)?;
    let parent_group_id = parse_node_id(args.parent_group_id)?;
    let destination_index = args
        .destination_index
        .try_into()
        .map_err(|_| CoreError::InvalidGroupMove)?;
    let handle = core.handle.as_mut().ok_or(CoreError::DatabaseLocked)?;
    if handle.database().get_group(&group_id).is_none()
        || handle.database().get_group(&parent_group_id).is_none()
    {
        return Err(CoreError::GroupNotFound);
    }
    if !handle
        .database_mut()
        .reposition_group(&group_id, &parent_group_id, destination_index)
    {
        return Err(CoreError::InvalidGroupMove);
    }

    core.touch_activity();
    Ok(EmptyResult {})
}

pub(super) fn execute(core: &mut KeelessCore, args: MoveGroupArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::MoveGroup(run(core, args)?))
}
