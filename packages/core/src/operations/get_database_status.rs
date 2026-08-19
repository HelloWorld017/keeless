use keeless_schema::{DatabaseStatusResult, GetDatabaseStatusArgs, OperationSuccess};

use crate::{DatabaseStatus, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore) -> DatabaseStatus {
    match (&core.selection, &core.handle) {
        (None, _) => DatabaseStatus::NotExist,
        (Some(selection), _) if !selection.exists => DatabaseStatus::NotExist,
        (_, Some(_)) => DatabaseStatus::Unlocked,
        _ => DatabaseStatus::Locked,
    }
}

pub(super) fn execute(
    core: &mut KeelessCore,
    _args: GetDatabaseStatusArgs,
) -> Result<OperationSuccess> {
    let status = run(core);
    let dirty = core.is_sync_dirty()
        || core.handle.as_ref().is_some_and(|handle| handle.is_dirty())
        || core
            .journal
            .as_ref()
            .is_some_and(|journal| journal.is_dirty());
    Ok(OperationSuccess::GetDatabaseStatus(DatabaseStatusResult {
        status,
        sync_status: core.sync_status(),
        dirty,
        sync_error: core.sync_error(),
    }))
}
