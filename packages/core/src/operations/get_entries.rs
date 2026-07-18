use keeless_schema::{EntriesResult, GetEntriesArgs, OperationSuccess};

use super::database_dto::all_entry_summaries;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore) -> Result<EntriesResult> {
    core.enforce_auto_lock();
    let entries = {
        let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
        all_entry_summaries(handle.database())
    };
    core.touch_activity();
    Ok(EntriesResult { entries })
}

pub(super) fn execute(core: &mut KeelessCore, _args: GetEntriesArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetEntries(run(core)?))
}
