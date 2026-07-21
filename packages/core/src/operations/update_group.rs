use keeless_schema::{EmptyResult, OperationSuccess, UpdateGroupArgs};

use crate::model::{parse_icon_reference, parse_node_id};
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: UpdateGroupArgs) -> Result<EmptyResult> {
    core.enforce_auto_lock();
    let group_id = parse_node_id(args.group_id)?;
    let name = args.name.trim();
    if name.is_empty() {
        return Err(CoreError::InvalidGroupName);
    }
    let handle = core.handle.as_mut().ok_or(CoreError::DatabaseLocked)?;
    if handle.database().get_group(&group_id).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    let icon = parse_icon_reference(handle.database(), &args.icon)?;
    let changed = handle.apply_update::<CoreError>(|database| {
        Ok(database.update_group(&group_id, name.to_string(), icon))
    })?;
    if changed {
        core.touch_activity();
    }
    Ok(EmptyResult {})
}

pub(super) fn execute(core: &mut KeelessCore, args: UpdateGroupArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::UpdateGroup(run(core, args)?))
}
