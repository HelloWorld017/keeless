use keeless_kdbx::NodeId;
use keeless_schema::{EntriesResult, GetEntryTemplatesArgs, OperationSuccess};

use crate::model::entry_summary;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore) -> Result<EntriesResult> {
    core.enforce_auto_lock();
    let entries = {
        let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
        let database = handle.database();
        database
            .entry_templates_uuid
            .map(NodeId::from_uuid)
            .filter(|group_id| database.get_group(group_id).is_some())
            .map(|group_id| {
                database
                    .get_entries_in_group(&group_id)
                    .into_iter()
                    .filter(|entry| entry.is_etm_template())
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
    _: GetEntryTemplatesArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetEntryTemplates(run(core)?))
}
