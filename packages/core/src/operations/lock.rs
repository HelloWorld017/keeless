use keeless_schema::{EmptyResult, LockArgs, OperationSuccess};

use crate::{KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore) {
    core.handle = None;
    core.credential = None;
    core.last_activity_ms = None;
}

pub(super) fn execute(core: &mut KeelessCore, _args: LockArgs) -> Result<OperationSuccess> {
    run(core);
    Ok(OperationSuccess::Lock(EmptyResult {}))
}
