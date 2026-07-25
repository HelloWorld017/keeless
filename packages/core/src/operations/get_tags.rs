use std::collections::BTreeMap;

use keeless_kdbx::TagQuery;
use keeless_schema::{GetTagsArgs, OperationSuccess, TagSummary, TagsResult};

use crate::features::tag_styles;
use crate::model::all_entries;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore) -> Result<TagsResult> {
    core.enforce_auto_lock();
    let tags = {
        let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
        let database = handle.database();
        let mut tags = TagQuery::query_entries(&all_entries(database, true))
            .into_iter()
            .map(|tag| (tag.name, tag.entry_count as u64))
            .collect::<BTreeMap<_, _>>();
        let styles = tag_styles::load(database).unwrap_or_default();
        for name in styles.keys() {
            tags.entry(name.clone()).or_default();
        }
        tags.into_iter()
            .map(|(name, entry_count)| {
                let style = styles.get(&name).cloned();
                let can_delete = style.is_some() && !tag_styles::is_used(database, &name);
                TagSummary {
                    name,
                    entry_count,
                    style,
                    can_delete,
                }
            })
            .collect()
    };
    core.touch_activity();
    Ok(TagsResult { tags })
}

pub(super) fn execute(core: &mut KeelessCore, _args: GetTagsArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetTags(run(core)?))
}
