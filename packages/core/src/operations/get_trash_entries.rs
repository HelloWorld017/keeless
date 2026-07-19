use keeless_kdbx::NodeId;
use keeless_schema::{EntriesResult, GetTrashEntriesArgs, OperationSuccess};

use crate::model::entry_summary;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore) -> Result<EntriesResult> {
    core.enforce_auto_lock();
    let entries = {
        let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
        let database = handle.database();
        database
            .recycle_bin_uuid
            .map(NodeId::from_uuid)
            .filter(|id| database.get_group(id).is_some())
            .map(|id| {
                database
                    .get_all_entries_in_group(&id)
                    .into_iter()
                    .map(entry_summary)
                    .collect()
            })
            .unwrap_or_default()
    };
    core.touch_activity();
    Ok(EntriesResult { entries })
}

pub(super) fn execute(
    core: &mut KeelessCore,
    _args: GetTrashEntriesArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetTrashEntries(run(core)?))
}
