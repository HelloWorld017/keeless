use keeless_schema::{DeleteGroupArgs, EmptyResult, OperationSuccess};

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: DeleteGroupArgs) -> Result<EmptyResult> {
    core.enforce_auto_lock();
    let group_id = parse_node_id(args.group_id)?;
    let handle = core.handle.as_mut().ok_or(CoreError::DatabaseLocked)?;
    let database = handle.database_mut();

    if database.get_group(&group_id).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    if database.root_group_id == Some(group_id) || database.is_recycle_bin(&group_id) {
        return Err(CoreError::InvalidGroupDelete);
    }

    database.create_recycle_bin();
    if database.remove_group(&group_id, true).is_none() {
        return Err(CoreError::InvalidGroupDelete);
    }

    core.touch_activity();
    Ok(EmptyResult {})
}

pub(super) fn execute(core: &mut KeelessCore, args: DeleteGroupArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::DeleteGroup(run(core, args)?))
}
