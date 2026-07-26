use keeless_schema::{EntriesResult, GetEntriesArgs, OperationSuccess};

use crate::model::all_entry_summaries;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: GetEntriesArgs) -> Result<EntriesResult> {
    let entries = {
        let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
        all_entry_summaries(handle.database(), args.exclude_trash)
    };
    core.touch_activity();
    Ok(EntriesResult { entries })
}

pub(super) fn execute(core: &mut KeelessCore, args: GetEntriesArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetEntries(run(core, args)?))
}
