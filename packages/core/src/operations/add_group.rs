use keeless_kdbx::{Group, NodeId};
use keeless_schema::{AddGroupArgs, AddGroupResult, OperationSuccess};

use crate::model::{node_id, parse_node_id};
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: AddGroupArgs) -> Result<AddGroupResult> {
    core.enforce_auto_lock();
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let parent_group_id = parse_node_id(args.parent_group_id)?;
    let handle = core.handle.as_mut().ok_or(CoreError::DatabaseLocked)?;
    if handle.database().get_group(&parent_group_id).is_none() {
        return Err(CoreError::GroupNotFound);
    }

    let id = NodeId::new_uuid();
    let mut group = Group::new(id);
    group.title = "Untitled Group".into();
    if !handle.database_mut().add_group(group, &parent_group_id) {
        return Err(CoreError::GroupNotFound);
    }
    core.touch_activity();
    Ok(AddGroupResult { id: node_id(id) })
}

pub(super) fn execute(core: &mut KeelessCore, args: AddGroupArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::AddGroup(run(core, args)?))
}
