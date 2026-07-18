//! Tag aggregation queries.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::db::Database;
use crate::model::entry::Entry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagResult {
    pub name: String,
    pub entry_count: usize,
}

pub struct TagQuery;

impl TagQuery {
    pub fn query_database(database: &Database) -> Vec<TagResult> {
        let entries = database.entries.values().collect::<Vec<_>>();
        Self::query_entries(&entries)
    }

    pub fn query_entries(entries: &[&Entry]) -> Vec<TagResult> {
        let mut counts = BTreeMap::<String, usize>::new();

        for entry in entries {
            let tags = entry
                .tags
                .iter()
                .map(|tag| tag.trim())
                .filter(|tag| !tag.is_empty())
                .collect::<BTreeSet<_>>();

            for tag in tags {
                *counts.entry(tag.to_string()).or_default() += 1;
            }
        }

        counts
            .into_iter()
            .map(|(name, entry_count)| TagResult { name, entry_count })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::core::node::NodeId;
    use crate::model::db::{Database, DatabaseVersion};

    fn entry(tags: &[&str]) -> Entry {
        let mut entry = Entry::new(NodeId::new_uuid());
        entry.tags = tags.iter().map(|tag| (*tag).to_string()).collect();
        entry
    }

    #[test]
    fn aggregates_unique_entries_and_sorts_tags() {
        let first = entry(&["work", "vpn", "work"]);
        let second = entry(&["email", "work"]);
        let third = entry(&["email"]);

        assert_eq!(
            TagQuery::query_entries(&[&first, &second, &third]),
            vec![
                TagResult {
                    name: "email".into(),
                    entry_count: 2,
                },
                TagResult {
                    name: "vpn".into(),
                    entry_count: 1,
                },
                TagResult {
                    name: "work".into(),
                    entry_count: 2,
                },
            ]
        );
    }

    #[test]
    fn trims_tags_skips_empty_values_and_preserves_case() {
        let entry = entry(&[" work ", "", "  ", "Work"]);

        assert_eq!(
            TagQuery::query_entries(&[&entry]),
            vec![
                TagResult {
                    name: "Work".into(),
                    entry_count: 1,
                },
                TagResult {
                    name: "work".into(),
                    entry_count: 1,
                },
            ]
        );
    }

    #[test]
    fn queries_current_database_entries_only() {
        let mut database = Database::new(DatabaseVersion::KDBX4);
        let mut current = entry(&["current"]);
        current.history.push(entry(&["history"]));
        database.entries.insert(current.id, current);

        assert_eq!(
            TagQuery::query_database(&database),
            vec![TagResult {
                name: "current".into(),
                entry_count: 1,
            }]
        );
    }
}
