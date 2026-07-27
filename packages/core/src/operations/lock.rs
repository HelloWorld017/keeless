use keeless_schema::{EmptyResult, LockArgs, OperationSuccess};

use crate::{KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore) {
    core.extensions.lock();
    core.handle = None;
    core.credential = None;
    core.last_activity_ms = None;
    core.journal = None;
    core.pending_sync_key = None;
    core.background_fetch = None;
    core.background_started_ms = None;
    if core.sync_status == crate::SyncStatus::Syncing {
        core.sync_status = crate::SyncStatus::Idle;
    }
}

pub(super) fn execute(core: &mut KeelessCore, _args: LockArgs) -> Result<OperationSuccess> {
    run(core);
    Ok(OperationSuccess::Lock(EmptyResult {}))
}
