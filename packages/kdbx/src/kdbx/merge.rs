//! Database merge engine
//!
//! Supports two-way merge and three-way merge with common ancestor.

use crate::model::db::database::Database;
use crate::model::entry::Entry;
use crate::model::core::node::{Node, NodeId};

/// Merge strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStrategy {
    /// Keep the existing entry when conflicts arise
    KeepExisting,
    /// Overwrite with the incoming entry
    Overwrite,
    /// Keep both (duplicate)
    KeepBoth,
    /// Use the newer version (based on last_modified timestamp)
    NewestWins,
}

/// Merge result summary
#[derive(Debug, Clone, Default)]
pub struct MergeResult {
    pub entries_added: usize,
    pub entries_modified: usize,
    pub entries_deleted: usize,
    pub groups_added: usize,
    pub groups_modified: usize,
    pub groups_deleted: usize,
    pub conflicts: Vec<MergeConflict>,
}

/// Describes a merge conflict.
#[derive(Debug, Clone)]
pub struct MergeConflict {
    pub node_id: NodeId,
    pub conflict_type: ConflictType,
    pub resolution: ConflictResolution,
}

/// Type of merge conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictType {
    /// Both versions modified the same entry
    EntryModified,
    /// Entry deleted in one version, modified in another
    EntryDeleteVsModify,
    /// Group structure conflict
    GroupModified,
}

/// How the conflict was resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictResolution {
    KeptExisting,
    TookIncoming,
    Duplicated,
    NewestUsed,
}

/// Database merger.
pub struct DatabaseMerger {
    strategy: MergeStrategy,
}

impl DatabaseMerger {
    pub fn new(strategy: MergeStrategy) -> Self {
        Self { strategy }
    }

    /// Two-way merge: merge source database into target database.
    pub fn merge(&self, target: &mut Database, source: &Database) -> MergeResult {
        let mut result = MergeResult::default();

        // Merge groups
        self.merge_groups(target, source, &mut result);

        // Merge entries
        self.merge_entries(target, source, &mut result);

        // Merge custom icons
        for (uuid, icon) in &source.custom_icons {
            if !target.custom_icons.contains_key(uuid) {
                target.custom_icons.insert(*uuid, icon.clone());
            }
        }

        // Merge deleted objects
        for deleted in &source.deleted_objects {
            if !target.deleted_objects.contains(deleted) {
                target.deleted_objects.push(*deleted);
            }
        }

        if result.entries_added + result.entries_modified + result.groups_added + result.groups_modified > 0 {
            target.mark_modified();
        }

        result
    }

    /// Three-way merge: merge source into target using base as common ancestor.
    ///
    /// This implements a proper three-way merge:
    /// - If only one side changed → take that change
    /// - If both sides changed → apply strategy
    /// - If neither changed → no-op
    pub fn merge_three_way(
        &self,
        target: &mut Database,
        source: &Database,
        base: &Database,
    ) -> MergeResult {
        let mut result = MergeResult::default();

        // Three-way merge for entries
        for (id, source_entry) in &source.entries {
            let in_base = base.entries.contains_key(id);
            let in_target = target.entries.contains_key(id);

            match (in_base, in_target) {
                (false, false) => {
                    // New in source only → add
                    target.entries.insert(*id, source_entry.clone());
                    result.entries_added += 1;
                }
                (false, true) => {
                    // Added independently in both → conflict
                    let target_entry = target.entries.get(id).expect("in_base&&in_target checked");
                    if entry_differs(target_entry, source_entry) {
                        result.conflicts.push(MergeConflict {
                            node_id: *id,
                            conflict_type: ConflictType::EntryModified,
                            resolution: self.resolve_conflict(target, source_entry, id),
                        });
                    }
                }
                (true, false) => {
                    // Deleted in target, exists in source
                    if source_entry.last_modified() > base.entries.get(id).map_or(0, |e| e.last_modified()) {
                        // Modified in source after base → delete-modify conflict
                        result.conflicts.push(MergeConflict {
                            node_id: *id,
                            conflict_type: ConflictType::EntryDeleteVsModify,
                            resolution: ConflictResolution::KeptExisting,
                        });
                    }
                    // Otherwise: both deleted or source didn't change → no-op
                }
                (true, true) => {
                    // Existed in base → check modifications
                    let base_entry = base.entries.get(id).expect("in_base&&in_target checked");
                    let target_entry = target.entries.get(id).expect("in_base&&in_target checked");
                    let source_modified = source_entry.last_modified() > base_entry.last_modified();
                    let target_modified = target_entry.last_modified() > base_entry.last_modified();

                    match (source_modified, target_modified) {
                        (true, false) => {
                            // Only source modified → take source
                            self.apply_entry_change(target, source_entry, id, &mut result);
                        }
                        (false, true) => {
                            // Only target modified → keep target (no-op)
                        }
                        (true, true) => {
                            // Both modified → conflict
                            if !entry_differs(target_entry, source_entry) {
                                // Same changes → no conflict
                            } else {
                                result.conflicts.push(MergeConflict {
                                    node_id: *id,
                                    conflict_type: ConflictType::EntryModified,
                                    resolution: self.resolve_conflict(target, source_entry, id),
                                });
                            }
                        }
                        (false, false) => {
                            // Neither modified → no-op
                        }
                    }
                }
            }
        }

        // Check for entries deleted in source
        let ids_to_check: Vec<NodeId> = target.entries.keys().copied().collect();
        for id in &ids_to_check {
            let in_base = base.entries.contains_key(id);
            let in_source = source.entries.contains_key(id);

            if in_base && !in_source {
                // Deleted in source
                let target_time = target.entries.get(id).map_or(0, Node::last_modified);
                let base_time = base.entries.get(id).map_or(0, Node::last_modified);
                let target_modified = target_time > base_time;
                if target_modified {
                    result.conflicts.push(MergeConflict {
                        node_id: *id,
                        conflict_type: ConflictType::EntryDeleteVsModify,
                        resolution: ConflictResolution::KeptExisting,
                    });
                } else {
                    target.entries.remove(id);
                    result.entries_deleted += 1;
                }
            }
        }

        // Three-way merge for groups (simplified)
        self.merge_groups(target, source, &mut result);

        if result.entries_added + result.entries_modified + result.groups_added > 0 {
            target.mark_modified();
        }

        result
    }

    fn merge_groups(&self, target: &mut Database, source: &Database, result: &mut MergeResult) {
        for (id, source_group) in &source.groups {
            if let Some(target_group) = target.groups.get_mut(id) {
                if source_group.last_modified() > target_group.last_modified() {
                    match self.strategy {
                        MergeStrategy::Overwrite | MergeStrategy::NewestWins => {
                            *target_group = source_group.clone();
                            result.groups_modified += 1;
                        }
                        MergeStrategy::KeepExisting | MergeStrategy::KeepBoth => {}
                    }
                }
            } else {
                target.groups.insert(*id, source_group.clone());
                result.groups_added += 1;
            }
        }
    }

    fn merge_entries(&self, target: &mut Database, source: &Database, result: &mut MergeResult) {
        for (id, source_entry) in &source.entries {
            if let Some(target_entry) = target.entries.get_mut(id) {
                if source_entry.last_modified() > target_entry.last_modified() {
                    match self.strategy {
                        MergeStrategy::Overwrite | MergeStrategy::NewestWins => {
                            *target_entry = source_entry.clone();
                            result.entries_modified += 1;
                        }
                        MergeStrategy::KeepExisting => {}
                        MergeStrategy::KeepBoth => {
                            let mut duplicate = source_entry.clone();
                            duplicate.id = NodeId::new_uuid();
                            target.entries.insert(duplicate.id, duplicate);
                            result.entries_added += 1;
                        }
                    }
                }
            } else {
                target.entries.insert(*id, source_entry.clone());
                result.entries_added += 1;
            }
        }
    }

    fn apply_entry_change(
        &self,
        target: &mut Database,
        source_entry: &Entry,
        id: &NodeId,
        result: &mut MergeResult,
    ) {
        match self.strategy {
            MergeStrategy::Overwrite | MergeStrategy::NewestWins => {
                if let Some(target_entry) = target.entries.get_mut(id) {
                    *target_entry = source_entry.clone();
                    result.entries_modified += 1;
                }
            }
            MergeStrategy::KeepExisting => {}
            MergeStrategy::KeepBoth => {
                let mut duplicate = source_entry.clone();
                duplicate.id = NodeId::new_uuid();
                target.entries.insert(duplicate.id, duplicate);
                result.entries_added += 1;
            }
        }
    }

    fn resolve_conflict(
        &self,
        target: &mut Database,
        source_entry: &Entry,
        id: &NodeId,
    ) -> ConflictResolution {
        match self.strategy {
            MergeStrategy::KeepExisting => ConflictResolution::KeptExisting,
            MergeStrategy::Overwrite => {
                if let Some(t) = target.entries.get_mut(id) {
                    *t = source_entry.clone();
                }
                ConflictResolution::TookIncoming
            }
            MergeStrategy::KeepBoth => {
                let mut duplicate = source_entry.clone();
                duplicate.id = NodeId::new_uuid();
                target.entries.insert(duplicate.id, duplicate);
                ConflictResolution::Duplicated
            }
            MergeStrategy::NewestWins => {
                let target_time = target.entries.get(id).map_or(0, |e| e.last_modified());
                if source_entry.last_modified() > target_time {
                    if let Some(t) = target.entries.get_mut(id) {
                        *t = source_entry.clone();
                    }
                    ConflictResolution::NewestUsed
                } else {
                    ConflictResolution::NewestUsed
                }
            }
        }
    }
}

/// Check if two entries have different content.
fn entry_differs(a: &Entry, b: &Entry) -> bool {
    a.title != b.title
        || a.url != b.url
        || a.username.as_str() != b.username.as_str()
        || a.notes.as_str() != b.notes.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::db::database::DatabaseVersion;
    

    fn make_entry_with_title(id: NodeId, title: &str) -> Entry {
        let mut e = Entry::new(id);
        e.title = title.to_string();
        e
    }

    fn make_entry_newer(id: NodeId, title: &str) -> Entry {
        let mut e = Entry::new(id);
        e.title = title.to_string();
        // Set modification time to future
        e.last_modification_time = crate::model::core::date::DateInstant::EpochMillis(
            e.last_modification_time.as_millis().unwrap_or(0) + 100_000
        );
        e
    }

    #[test]
    fn test_merge_new_entry() {
        let mut target = Database::new(DatabaseVersion::KDBX4);
        let mut source = Database::new(DatabaseVersion::KDBX4);

        let entry_id = NodeId::new_uuid();
        let entry = make_entry_with_title(entry_id, "New Entry");
        source.entries.insert(entry_id, entry);

        let merger = DatabaseMerger::new(MergeStrategy::Overwrite);
        let result = merger.merge(&mut target, &source);

        assert_eq!(result.entries_added, 1);
        assert_eq!(target.entries.len(), 1);
    }

    #[test]
    fn test_merge_keep_existing() {
        let mut target = Database::new(DatabaseVersion::KDBX4);
        let mut source = Database::new(DatabaseVersion::KDBX4);

        let entry_id = NodeId::new_uuid();
        let e1 = make_entry_with_title(entry_id, "Original");
        target.entries.insert(entry_id, e1);

        let e2 = make_entry_with_title(entry_id, "Modified");
        source.entries.insert(entry_id, e2);

        let merger = DatabaseMerger::new(MergeStrategy::KeepExisting);
        let _result = merger.merge(&mut target, &source);

        assert_eq!(target.entries.get(&entry_id).unwrap().title, "Original");
    }

    #[test]
    fn test_merge_overwrite() {
        let mut target = Database::new(DatabaseVersion::KDBX4);
        let mut source = Database::new(DatabaseVersion::KDBX4);

        let entry_id = NodeId::new_uuid();
        target.entries.insert(entry_id, make_entry_with_title(entry_id, "Old"));
        source.entries.insert(entry_id, make_entry_newer(entry_id, "New"));

        let merger = DatabaseMerger::new(MergeStrategy::Overwrite);
        let result = merger.merge(&mut target, &source);

        assert_eq!(result.entries_modified, 1);
        assert_eq!(target.entries.get(&entry_id).unwrap().title, "New");
    }

    #[test]
    fn test_merge_keep_both() {
        let mut target = Database::new(DatabaseVersion::KDBX4);
        let mut source = Database::new(DatabaseVersion::KDBX4);

        let entry_id = NodeId::new_uuid();
        target.entries.insert(entry_id, make_entry_with_title(entry_id, "Original"));
        source.entries.insert(entry_id, make_entry_newer(entry_id, "Modified"));

        let merger = DatabaseMerger::new(MergeStrategy::KeepBoth);
        let _result = merger.merge(&mut target, &source);

        assert_eq!(target.entries.len(), 2); // Original + duplicate
        assert!(target.entries.contains_key(&entry_id));
    }

    #[test]
    fn test_three_way_merge_source_only_change() {
        let mut base = Database::new(DatabaseVersion::KDBX4);
        let mut target = Database::new(DatabaseVersion::KDBX4);
        let mut source = Database::new(DatabaseVersion::KDBX4);

        let entry_id = NodeId::new_uuid();
        base.entries.insert(entry_id, make_entry_with_title(entry_id, "Base"));
        target.entries.insert(entry_id, make_entry_with_title(entry_id, "Base"));
        source.entries.insert(entry_id, make_entry_newer(entry_id, "Source Modified"));

        let merger = DatabaseMerger::new(MergeStrategy::Overwrite);
        let result = merger.merge_three_way(&mut target, &source, &base);

        assert_eq!(result.conflicts.len(), 0);
        assert_eq!(target.entries.get(&entry_id).unwrap().title, "Source Modified");
    }

    #[test]
    fn test_three_way_merge_no_change() {
        let mut base = Database::new(DatabaseVersion::KDBX4);
        let mut target = Database::new(DatabaseVersion::KDBX4);
        let mut source = Database::new(DatabaseVersion::KDBX4);

        let entry_id = NodeId::new_uuid();
        let entry = make_entry_with_title(entry_id, "Same");
        base.entries.insert(entry_id, entry.clone());
        target.entries.insert(entry_id, entry.clone());
        source.entries.insert(entry_id, entry);

        let merger = DatabaseMerger::new(MergeStrategy::Overwrite);
        let result = merger.merge_three_way(&mut target, &source, &base);

        assert_eq!(result.entries_modified, 0);
        assert_eq!(result.conflicts.len(), 0);
    }

    #[test]
    fn test_three_way_merge_new_in_source() {
        let base = Database::new(DatabaseVersion::KDBX4);
        let mut target = Database::new(DatabaseVersion::KDBX4);
        let mut source = Database::new(DatabaseVersion::KDBX4);

        let new_id = NodeId::new_uuid();
        source.entries.insert(new_id, make_entry_with_title(new_id, "New Entry"));

        let merger = DatabaseMerger::new(MergeStrategy::Overwrite);
        let result = merger.merge_three_way(&mut target, &source, &base);

        assert_eq!(result.entries_added, 1);
        assert!(target.entries.contains_key(&new_id));
    }

    #[test]
    fn test_entry_differs() {
        let id = NodeId::new_uuid();
        let a = make_entry_with_title(id, "A");
        let b = make_entry_with_title(id, "B");
        assert!(entry_differs(&a, &b));

        let c = make_entry_with_title(id, "Same");
        let d = make_entry_with_title(id, "Same");
        assert!(!entry_differs(&c, &d));
    }
}
