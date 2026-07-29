use std::collections::{HashSet, VecDeque};

use keeless_kdbx::{FuzzySearchHelper, NodeId, SearchQuery, TagQuery};
use keeless_schema::{OperationSuccess, SearchFuzzyArgs, SearchFuzzyResult, TagSummary};

use crate::features::tag_styles;
use crate::model::{all_entries, entry_summary, group_hierarchy, node_id};
use crate::{CoreError, KeelessCore, Result};

const RESULT_LIMIT: usize = 8;

pub(crate) fn run(core: &mut KeelessCore, args: SearchFuzzyArgs) -> Result<SearchFuzzyResult> {
    let key = core
        .credential
        .as_ref()
        .map(|credential| credential.restore_key())
        .transpose()?;
    let result = {
        let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
        let database = handle.database();
        let query = SearchQuery::parse(&args.query);
        let visible_entries = all_entries(database, true);
        let mut helper = FuzzySearchHelper::new(&query);
        let entries = helper
            .search_entries(database, &visible_entries, key.as_ref(), &query)?
            .into_iter()
            .take(RESULT_LIMIT)
            .filter_map(|result| database.get_entry(&result.entry_id))
            .map(entry_summary)
            .collect();

        let excluded_groups = recycle_group_ids(database);
        let hierarchy = group_hierarchy(database)?;
        let mut groups = hierarchy
            .groups
            .into_iter()
            .enumerate()
            .filter(|(_, group)| {
                group.id != hierarchy.root_group_id
                    && !excluded_groups.contains(&group_id_from_item(database, group))
            })
            .filter_map(|(index, group)| {
                helper.score(&group.name).map(|score| (score, index, group))
            })
            .collect::<Vec<_>>();
        groups.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        let groups = groups
            .into_iter()
            .take(RESULT_LIMIT)
            .map(|(_, _, group)| group)
            .collect();

        let styles = tag_styles::load(database).unwrap_or_default();
        let mut tags = TagQuery::query_entries(&visible_entries)
            .into_iter()
            .enumerate()
            .filter_map(|(index, tag)| {
                helper.score(&tag.name).map(|score| {
                    (
                        score,
                        index,
                        TagSummary {
                            can_delete: styles
                                .get(&tag.name)
                                .is_some_and(|_| !tag_styles::is_used(database, &tag.name)),
                            style: styles.get(&tag.name).cloned(),
                            name: tag.name,
                            entry_count: tag.entry_count as u64,
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        tags.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        let tags = tags
            .into_iter()
            .take(RESULT_LIMIT)
            .map(|(_, _, tag)| tag)
            .collect();

        SearchFuzzyResult {
            entries,
            groups,
            tags,
            trash_matches: helper.has_free_text() && helper.score("Trash Recycle Bin").is_some(),
            filter_tokens: query.filter_tokens,
        }
    };
    core.touch_activity();
    Ok(result)
}

fn group_id_from_item(
    database: &keeless_kdbx::Database,
    item: &keeless_schema::GroupHierarchyItem,
) -> NodeId {
    database
        .groups
        .values()
        .find(|group| node_id(group.id) == item.id)
        .map(|group| group.id)
        .expect("group hierarchy items come from the database")
}

fn recycle_group_ids(database: &keeless_kdbx::Database) -> HashSet<NodeId> {
    let Some(recycle_bin_id) = database.recycle_bin_uuid.map(NodeId::from_uuid) else {
        return HashSet::new();
    };
    let mut excluded = HashSet::new();
    let mut pending = VecDeque::from([recycle_bin_id]);
    while let Some(group_id) = pending.pop_front() {
        if excluded.insert(group_id)
            && let Some(group) = database.get_group(&group_id)
        {
            pending.extend(group.child_group_ids.iter().copied());
        }
    }
    excluded
}

pub(super) fn execute(core: &mut KeelessCore, args: SearchFuzzyArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::SearchFuzzy(run(core, args)?))
}
