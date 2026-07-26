use std::collections::HashSet;

use keeless_kdbx::{SearchHelper, SearchParameters};
use keeless_schema::{EntriesResult, OperationSuccess, SearchEntriesArgs};

use crate::model::{all_entries, entry_summary};
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: SearchEntriesArgs) -> Result<EntriesResult> {
    let key = core
        .credential
        .as_ref()
        .map(|credential| credential.restore_key())
        .transpose()?;
    let entries = {
        let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
        let database = handle.database();
        let visible_ids = all_entries(database, true)
            .into_iter()
            .map(|entry| entry.id)
            .collect::<HashSet<_>>();
        SearchHelper::search_database(database, key.as_ref(), &SearchParameters::new(&args.query))?
            .into_iter()
            .filter(|result| visible_ids.contains(&result.entry_id))
            .filter_map(|result| database.get_entry(&result.entry_id))
            .map(entry_summary)
            .collect()
    };
    core.touch_activity();
    Ok(EntriesResult { entries })
}

pub(super) fn execute(core: &mut KeelessCore, args: SearchEntriesArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::SearchEntries(run(core, args)?))
}
