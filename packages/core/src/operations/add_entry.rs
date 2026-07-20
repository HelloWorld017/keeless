use keeless_kdbx::{Entry, NodeId};
use keeless_schema::{AddEntryArgs, AddEntryResult, OperationSuccess};

use crate::model::{node_id, parse_node_id};
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: AddEntryArgs) -> Result<AddEntryResult> {
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
    let entry = Entry::new(id);
    if !handle.database_mut().add_entry(entry, &parent_group_id) {
        return Err(CoreError::GroupNotFound);
    }
    core.touch_activity();
    Ok(AddEntryResult { id: node_id(id) })
}

pub(super) fn execute(core: &mut KeelessCore, args: AddEntryArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::AddEntry(run(core, args)?))
}
