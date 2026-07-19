use keeless_kdbx::TagQuery;
use keeless_schema::{GetTagsArgs, OperationSuccess, TagSummary, TagsResult};

use crate::model::all_entries;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore) -> Result<TagsResult> {
    core.enforce_auto_lock();
    let tags = {
        let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
        TagQuery::query_entries(&all_entries(handle.database(), true))
            .into_iter()
            .map(|tag| TagSummary {
                name: tag.name,
                entry_count: tag.entry_count as u64,
            })
            .collect()
    };
    core.touch_activity();
    Ok(TagsResult { tags })
}

pub(super) fn execute(core: &mut KeelessCore, _args: GetTagsArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetTags(run(core)?))
}
