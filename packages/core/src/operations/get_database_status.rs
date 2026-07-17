use keeless_schema::{DatabaseStatusResult, GetDatabaseStatusArgs, OperationSuccess};

use crate::{DatabaseStatus, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore) -> DatabaseStatus {
    core.enforce_auto_lock();
    let status = match (&core.selection, &core.handle) {
        (None, _) => DatabaseStatus::NotExist,
        (Some(selection), _) if !selection.exists => DatabaseStatus::NotExist,
        (_, Some(_)) => DatabaseStatus::Unlocked,
        _ => DatabaseStatus::Locked,
    };
    core.touch_activity();
    status
}

pub(super) fn execute(
    core: &mut KeelessCore,
    _args: GetDatabaseStatusArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetDatabaseStatus(DatabaseStatusResult {
        status: run(core),
    }))
}
