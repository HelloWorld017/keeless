use keeless_schema::{EntriesResult, GetTagEntriesArgs, OperationSuccess};

use crate::model::{all_entries, entry_summary};
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: GetTagEntriesArgs) -> Result<EntriesResult> {
    let entries = {
        let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
        all_entries(handle.database(), true)
            .into_iter()
            .filter(|entry| {
                entry
                    .tags
                    .iter()
                    .map(|tag| tag.trim())
                    .filter(|tag| !tag.is_empty())
                    .any(|tag| tag == args.tag)
            })
            .map(entry_summary)
            .collect()
    };
    core.touch_activity();
    Ok(EntriesResult { entries })
}

pub(super) fn execute(core: &mut KeelessCore, args: GetTagEntriesArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetTagEntries(run(core, args)?))
}
