use keeless_kdbx::kdbx::template;
use keeless_schema::{EntryDetailResult, GetEntryDetailArgs, OperationSuccess};

use crate::model::{entry_detail, parse_node_id};
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: GetEntryDetailArgs) -> Result<EntryDetailResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let entry_id = parse_node_id(args.entry_id)?;
    let result = {
        let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
        let database = handle.database();
        let entry = database
            .get_entry(&entry_id)
            .ok_or(CoreError::EntryNotFound)?;
        let mut detail = entry_detail(database, entry);
        detail.is_template = template::is_template(database, &entry_id);
        detail
    };
    core.touch_activity();
    Ok(result)
}

pub(super) fn execute(
    core: &mut KeelessCore,
    args: GetEntryDetailArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetEntryDetail(Box::new(run(core, args)?)))
}
