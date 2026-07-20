use keeless_schema::{EmptyResult, OperationSuccess, RenameGroupArgs};

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: RenameGroupArgs) -> Result<EmptyResult> {
    core.enforce_auto_lock();
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let group_id = parse_node_id(args.group_id)?;
    let name = args.name.trim();
    if name.is_empty() {
        return Err(CoreError::InvalidGroupName);
    }
    let handle = core.handle.as_mut().ok_or(CoreError::DatabaseLocked)?;
    if handle.database().get_group(&group_id).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    if !handle
        .database_mut()
        .rename_group(&group_id, name.to_string())
    {
        return Err(CoreError::GroupNotFound);
    }
    core.touch_activity();
    Ok(EmptyResult {})
}

pub(super) fn execute(core: &mut KeelessCore, args: RenameGroupArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::RenameGroup(run(core, args)?))
}
