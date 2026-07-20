use keeless_schema::{DeleteEntryArgs, EmptyResult, OperationSuccess};

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: DeleteEntryArgs) -> Result<EmptyResult> {
    core.enforce_auto_lock();
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let entry_id = parse_node_id(args.entry_id)?;
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    let database = handle.database();
    if database.get_entry(&entry_id).is_none() {
        return Err(CoreError::EntryNotFound);
    }
    let in_recycle_bin = database.is_entry_in_recycle_bin(&entry_id);
    if args.permanent && !in_recycle_bin {
        return Err(CoreError::InvalidEntryDelete);
    }
    if !args.permanent && in_recycle_bin {
        return Err(CoreError::InvalidEntryDelete);
    }
    let changed = core
        .handle
        .as_mut()
        .ok_or(CoreError::DatabaseLocked)?
        .apply_update::<CoreError>(|database| {
            Ok(database.delete_entry(&entry_id, args.permanent))
        })?;
    if !changed {
        return Err(CoreError::InvalidEntryDelete);
    }
    if changed {
        core.touch_activity();
    }
    Ok(EmptyResult {})
}

pub(super) fn execute(core: &mut KeelessCore, args: DeleteEntryArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::DeleteEntry(run(core, args)?))
}
