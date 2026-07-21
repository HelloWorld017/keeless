use keeless_kdbx::kdbx::template;
use keeless_schema::{EntriesResult, GetEntryTemplatesArgs, OperationSuccess};

use crate::model::entry_summary;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore) -> Result<EntriesResult> {
    core.enforce_auto_lock();
    let entries = {
        let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
        let database = handle.database();
        template::entries(database)
            .into_iter()
            .map(entry_summary)
            .collect()
    };
    core.touch_activity();
    Ok(EntriesResult { entries })
}

pub(super) fn execute(
    core: &mut KeelessCore,
    _: GetEntryTemplatesArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetEntryTemplates(run(core)?))
}
