use keeless_schema::{EntriesResult, GetGroupEntriesArgs, OperationSuccess};

use crate::model::{entry_summary, parse_node_id};
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: GetGroupEntriesArgs) -> Result<EntriesResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let group_id = parse_node_id(args.group_id)?;
    let entries = {
        let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
        let database = handle.database();
        if database.get_group(&group_id).is_none() {
            return Err(CoreError::GroupNotFound);
        }
        database
            .get_entries_in_group(&group_id)
            .into_iter()
            .map(entry_summary)
            .collect()
    };
    core.touch_activity();
    Ok(EntriesResult { entries })
}

pub(super) fn execute(
    core: &mut KeelessCore,
    args: GetGroupEntriesArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetGroupEntries(run(core, args)?))
}
