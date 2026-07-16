//! Database merge engine
//!
//! Supports two-way merge and three-way merge with common ancestor.

use crate::model::core::node::{Node, NodeId};
use crate::model::db::database::Database;
use crate::model::entry::Entry;
use crate::model::group::Group;

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

        self.merge_groups(target, source, &mut result);
        self.merge_entries(target, source, &mut result);

        for deleted in &source.deleted_objects {
            if source.entries.contains_key(&deleted.id) || source.groups.contains_key(&deleted.id) {
                continue;
            }
            if let Some(entry) = target.entries.get(&deleted.id) {
                if deleted.deletion_time >= entry.last_modified() {
                    remove_entry(target, &deleted.id);
                    result.entries_deleted += 1;
                } else {
                    result.conflicts.push(MergeConflict {
                        node_id: deleted.id,
                        conflict_type: ConflictType::EntryDeleteVsModify,
                        resolution: ConflictResolution::KeptExisting,
                    });
                }
            } else if target.groups.contains_key(&deleted.id)
                && target.root_group_id != Some(deleted.id)
            {
                let modified = target
                    .groups
                    .get(&deleted.id)
                    .map_or(0, Node::last_modified);
                if deleted.deletion_time >= modified {
                    let (groups, entries) = remove_group(target, &deleted.id);
                    result.groups_deleted += groups;
                    result.entries_deleted += entries;
                }
            }
        }

        // Merge custom icons
        for (uuid, icon) in &source.custom_icons {
            if !target.custom_icons.contains_key(uuid) {
                target.custom_icons.insert(*uuid, icon.clone());
            }
        }

        // Merge deleted objects
        for deleted in &source.deleted_objects {
            if let Some(existing) = target
                .deleted_objects
                .iter_mut()
                .find(|item| item.id == deleted.id)
            {
                if deleted.deletion_time > existing.deletion_time {
                    *existing = deleted.clone();
                }
            } else {
                target.deleted_objects.push(deleted.clone());
            }
        }

        if merge_changed(&result) {
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

        self.merge_groups_three_way(target, source, base, &mut result);

        for (id, source_entry) in &source.entries {
            match (base.entries.get(id), target.entries.get(id)) {
                (None, None) => {
                    add_entry_from(target, source, source_entry.clone(), *id);
                    result.entries_added += 1;
                }
                (None, Some(target_entry)) => {
                    if entry_differs(target_entry, source_entry) {
                        let resolution = self.resolve_entry_conflict(
                            target,
                            source,
                            source_entry,
                            id,
                            &mut result,
                        );
                        result.conflicts.push(MergeConflict {
                            node_id: *id,
                            conflict_type: ConflictType::EntryModified,
                            resolution,
                        });
                    }
                }
                (Some(base_entry), None) => {
                    if entry_differs(source_entry, base_entry) {
                        let deletion_time = deleted_time(target, id).unwrap_or(0);
                        let take_source = match self.strategy {
                            MergeStrategy::Overwrite => true,
                            MergeStrategy::NewestWins => {
                                source_entry.last_modified() > deletion_time
                            }
                            MergeStrategy::KeepExisting | MergeStrategy::KeepBoth => false,
                        };
                        let resolution = if take_source {
                            add_entry_from(target, source, source_entry.clone(), *id);
                            clear_deleted(target, id);
                            result.entries_added += 1;
                            ConflictResolution::TookIncoming
                        } else if self.strategy == MergeStrategy::KeepBoth {
                            let mut duplicate = source_entry.clone();
                            duplicate.id = NodeId::new_uuid();
                            add_entry_from(target, source, duplicate, *id);
                            result.entries_added += 1;
                            ConflictResolution::Duplicated
                        } else {
                            ConflictResolution::KeptExisting
                        };
                        result.conflicts.push(MergeConflict {
                            node_id: *id,
                            conflict_type: ConflictType::EntryDeleteVsModify,
                            resolution,
                        });
                    }
                }
                (Some(base_entry), Some(target_entry)) => {
                    let source_modified = entry_differs(source_entry, base_entry);
                    let target_modified = entry_differs(target_entry, base_entry);

                    match (source_modified, target_modified) {
                        (true, false) => {
                            *target.entries.get_mut(id).expect("entry exists") =
                                source_entry.clone();
                            result.entries_modified += 1;
                        }
                        (false, true) => {}
                        (true, true) => {
                            if entry_differs(target_entry, source_entry) {
                                let resolution = self.resolve_entry_conflict(
                                    target,
                                    source,
                                    source_entry,
                                    id,
                                    &mut result,
                                );
                                result.conflicts.push(MergeConflict {
                                    node_id: *id,
                                    conflict_type: ConflictType::EntryModified,
                                    resolution,
                                });
                            }
                        }
                        (false, false) => {}
                    }
                }
            }
        }

        let ids_to_check: Vec<NodeId> = target.entries.keys().copied().collect();
        for id in &ids_to_check {
            if let (Some(base_entry), None) = (base.entries.get(id), source.entries.get(id)) {
                let target_entry = &target.entries[id];
                let target_modified = entry_differs(target_entry, base_entry);
                if target_modified {
                    let deletion_time = deleted_time(source, id).unwrap_or(0);
                    let delete = match self.strategy {
                        MergeStrategy::Overwrite => true,
                        MergeStrategy::NewestWins => deletion_time > target_entry.last_modified(),
                        MergeStrategy::KeepExisting | MergeStrategy::KeepBoth => false,
                    };
                    if delete {
                        remove_entry(target, id);
                        result.entries_deleted += 1;
                    }
                    result.conflicts.push(MergeConflict {
                        node_id: *id,
                        conflict_type: ConflictType::EntryDeleteVsModify,
                        resolution: if delete {
                            ConflictResolution::TookIncoming
                        } else {
                            ConflictResolution::KeptExisting
                        },
                    });
                } else {
                    remove_entry(target, id);
                    result.entries_deleted += 1;
                }
            }
        }

        merge_deleted_objects(target, source);

        if merge_changed(&result) {
            target.mark_modified();
        }

        result
    }

    fn merge_groups(&self, target: &mut Database, source: &Database, result: &mut MergeResult) {
        if target.root_group_id.is_none() {
            target.root_group_id = source.root_group_id;
        }
        for (id, source_group) in &source.groups {
            if let Some(target_group) = target.groups.get_mut(id) {
                if source_group.last_modified() > target_group.last_modified() {
                    match self.strategy {
                        MergeStrategy::Overwrite | MergeStrategy::NewestWins => {
                            copy_group_content(target_group, source_group);
                            result.groups_modified += 1;
                        }
                        MergeStrategy::KeepExisting | MergeStrategy::KeepBoth => {}
                    }
                }
            } else if source.root_group_id != Some(*id) || target.root_group_id == Some(*id) {
                let mut group = source_group.clone();
                group.child_group_ids.clear();
                group.child_entry_ids.clear();
                target.groups.insert(*id, group);
                result.groups_added += 1;
            }
        }
        let added: Vec<NodeId> = source
            .groups
            .keys()
            .filter(|id| target.groups.contains_key(id) && source.root_group_id != Some(**id))
            .copied()
            .collect();
        for id in added {
            if group_parent(source, &id).is_some() && group_parent(target, &id).is_none() {
                attach_group_from(target, source, id);
            }
        }
    }

    fn merge_groups_three_way(
        &self,
        target: &mut Database,
        source: &Database,
        base: &Database,
        result: &mut MergeResult,
    ) {
        if target.root_group_id.is_none() {
            target.root_group_id = source.root_group_id;
        }
        for (id, source_group) in &source.groups {
            match (base.groups.get(id), target.groups.get(id)) {
                (None, None) => {
                    if source.root_group_id != Some(*id) || target.root_group_id == Some(*id) {
                        let mut group = source_group.clone();
                        group.child_group_ids.clear();
                        group.child_entry_ids.clear();
                        target.groups.insert(*id, group);
                        result.groups_added += 1;
                    }
                }
                (None, Some(target_group)) => {
                    if group_content_differs(target_group, source_group) {
                        result.conflicts.push(MergeConflict {
                            node_id: *id,
                            conflict_type: ConflictType::GroupModified,
                            resolution: ConflictResolution::KeptExisting,
                        });
                    }
                }
                (Some(base_group), Some(target_group)) => {
                    let source_modified = group_content_differs(source_group, base_group);
                    let target_modified = group_content_differs(target_group, base_group);
                    if source_modified && !target_modified {
                        copy_group_content(
                            target.groups.get_mut(id).expect("group exists"),
                            source_group,
                        );
                        result.groups_modified += 1;
                    } else if source_modified
                        && target_modified
                        && group_content_differs(target_group, source_group)
                    {
                        let take_source = matches!(self.strategy, MergeStrategy::Overwrite)
                            || (self.strategy == MergeStrategy::NewestWins
                                && source_group.last_modified() > target_group.last_modified());
                        if take_source {
                            copy_group_content(
                                target.groups.get_mut(id).expect("group exists"),
                                source_group,
                            );
                            result.groups_modified += 1;
                        }
                        result.conflicts.push(MergeConflict {
                            node_id: *id,
                            conflict_type: ConflictType::GroupModified,
                            resolution: if take_source {
                                ConflictResolution::TookIncoming
                            } else {
                                ConflictResolution::KeptExisting
                            },
                        });
                    }
                }
                (Some(base_group), None) => {
                    if group_content_differs(source_group, base_group) {
                        let deletion_time = deleted_time(target, id).unwrap_or(0);
                        let take_source = self.strategy == MergeStrategy::Overwrite
                            || (self.strategy == MergeStrategy::NewestWins
                                && source_group.last_modified() > deletion_time);
                        if take_source {
                            let mut group = source_group.clone();
                            group.child_group_ids.clear();
                            group.child_entry_ids.clear();
                            target.groups.insert(*id, group);
                            attach_group_from(target, source, *id);
                            clear_deleted(target, id);
                            result.groups_added += 1;
                        }
                        result.conflicts.push(MergeConflict {
                            node_id: *id,
                            conflict_type: ConflictType::GroupModified,
                            resolution: if take_source {
                                ConflictResolution::TookIncoming
                            } else {
                                ConflictResolution::KeptExisting
                            },
                        });
                    }
                }
            }
        }

        let added: Vec<NodeId> = source
            .groups
            .keys()
            .filter(|id| !base.groups.contains_key(id) && target.groups.contains_key(id))
            .copied()
            .collect();
        for id in added {
            attach_group_from(target, source, id);
        }

        let common: Vec<NodeId> = source
            .groups
            .keys()
            .filter(|id| base.groups.contains_key(id) && target.groups.contains_key(id))
            .copied()
            .collect();
        for id in common {
            if Some(id) == target.root_group_id {
                continue;
            }
            let base_parent = group_parent(base, &id);
            let source_parent = group_parent(source, &id);
            let target_parent = group_parent(target, &id);
            let source_changed = source_parent != base_parent;
            let target_changed = target_parent != base_parent;
            if source_changed && (!target_changed || source_parent == target_parent) {
                move_group_from(target, source, id);
            } else if source_changed && target_changed && source_parent != target_parent {
                let take_source = matches!(self.strategy, MergeStrategy::Overwrite)
                    || (self.strategy == MergeStrategy::NewestWins
                        && source.groups[&id].location_changed.as_millis().unwrap_or(0)
                            > target.groups[&id].location_changed.as_millis().unwrap_or(0));
                if take_source {
                    move_group_from(target, source, id);
                }
                result.conflicts.push(MergeConflict {
                    node_id: id,
                    conflict_type: ConflictType::GroupModified,
                    resolution: if take_source {
                        ConflictResolution::TookIncoming
                    } else {
                        ConflictResolution::KeptExisting
                    },
                });
            }
        }

        let deleted_groups: Vec<NodeId> = target
            .groups
            .keys()
            .filter(|id| base.groups.contains_key(id) && !source.groups.contains_key(id))
            .copied()
            .collect();
        for id in deleted_groups {
            if !target.groups.contains_key(&id) || target.root_group_id == Some(id) {
                continue;
            }
            let target_modified = group_content_differs(&target.groups[&id], &base.groups[&id])
                || group_parent(target, &id) != group_parent(base, &id);
            let deletion_time = deleted_time(source, &id).unwrap_or(0);
            let delete = !target_modified
                || self.strategy == MergeStrategy::Overwrite
                || (self.strategy == MergeStrategy::NewestWins
                    && deletion_time > target.groups[&id].last_modified());
            if delete {
                let (groups, entries) = remove_group(target, &id);
                result.groups_deleted += groups;
                result.entries_deleted += entries;
            } else {
                result.conflicts.push(MergeConflict {
                    node_id: id,
                    conflict_type: ConflictType::GroupModified,
                    resolution: ConflictResolution::KeptExisting,
                });
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
                            add_entry_from(target, source, duplicate, *id);
                            result.entries_added += 1;
                        }
                    }
                }
            } else {
                add_entry_from(target, source, source_entry.clone(), *id);
                result.entries_added += 1;
            }
        }
    }

    fn resolve_entry_conflict(
        &self,
        target: &mut Database,
        source: &Database,
        source_entry: &Entry,
        id: &NodeId,
        result: &mut MergeResult,
    ) -> ConflictResolution {
        match self.strategy {
            MergeStrategy::KeepExisting => ConflictResolution::KeptExisting,
            MergeStrategy::Overwrite => {
                if let Some(t) = target.entries.get_mut(id) {
                    *t = source_entry.clone();
                }
                result.entries_modified += 1;
                ConflictResolution::TookIncoming
            }
            MergeStrategy::KeepBoth => {
                let mut duplicate = source_entry.clone();
                duplicate.id = NodeId::new_uuid();
                add_entry_from(target, source, duplicate, *id);
                result.entries_added += 1;
                ConflictResolution::Duplicated
            }
            MergeStrategy::NewestWins => {
                let target_time = target.entries.get(id).map_or(0, |e| e.last_modified());
                if source_entry.last_modified() > target_time {
                    if let Some(t) = target.entries.get_mut(id) {
                        *t = source_entry.clone();
                    }
                    result.entries_modified += 1;
                }
                ConflictResolution::NewestUsed
            }
        }
    }
}

/// Check if two entries have different content.
fn entry_differs(a: &Entry, b: &Entry) -> bool {
    a != b
}

fn group_content_differs(a: &Group, b: &Group) -> bool {
    let mut a = a.clone();
    let mut b = b.clone();
    a.child_group_ids.clear();
    a.child_entry_ids.clear();
    b.child_group_ids.clear();
    b.child_entry_ids.clear();
    a != b
}

fn copy_group_content(target: &mut Group, source: &Group) {
    let child_groups = std::mem::take(&mut target.child_group_ids);
    let child_entries = std::mem::take(&mut target.child_entry_ids);
    *target = source.clone();
    target.child_group_ids = child_groups;
    target.child_entry_ids = child_entries;
}

fn group_parent(db: &Database, id: &NodeId) -> Option<NodeId> {
    db.groups
        .iter()
        .find_map(|(parent_id, group)| group.child_group_ids.contains(id).then_some(*parent_id))
}

fn source_parent_in_target(target: &Database, parent: NodeId) -> Option<NodeId> {
    if target.groups.contains_key(&parent) {
        Some(parent)
    } else {
        target.root_group_id
    }
}

fn attach_group_from(target: &mut Database, source: &Database, id: NodeId) -> bool {
    let Some(source_parent) = group_parent(source, &id) else {
        return false;
    };
    let Some(mut parent) = source_parent_in_target(target, source_parent) else {
        return false;
    };
    if group_parent(target, &id).is_some() && group_contains(target, &id, &parent) {
        return false;
    }
    if group_parent(target, &id).is_none() && group_contains(target, &id, &parent) {
        let Some(root_id) = target.root_group_id else {
            return false;
        };
        parent = root_id;
    }
    for group in target.groups.values_mut() {
        group.child_group_ids.retain(|child| child != &id);
    }
    if let Some(group) = target.groups.get_mut(&parent) {
        group.add_child_group(id);
        true
    } else {
        false
    }
}

fn move_group_from(target: &mut Database, source: &Database, id: NodeId) {
    attach_group_from(target, source, id);
}

fn group_contains(db: &Database, ancestor: &NodeId, descendant: &NodeId) -> bool {
    let mut stack = vec![*ancestor];
    while let Some(id) = stack.pop() {
        if id == *descendant {
            return true;
        }
        if let Some(group) = db.groups.get(&id) {
            stack.extend(group.child_group_ids.iter().copied());
        }
    }
    false
}

fn add_entry_from(target: &mut Database, source: &Database, entry: Entry, source_id: NodeId) {
    let source_parent = source.find_parent_group_of_entry(&source_id);
    let parent = source_parent
        .and_then(|id| source_parent_in_target(target, id))
        .or(target.root_group_id);
    let id = entry.id;
    target.entries.insert(id, entry);
    if let Some(parent) = parent.and_then(|id| target.groups.get_mut(&id)) {
        parent.add_child_entry(id);
    }
}

fn remove_entry(target: &mut Database, id: &NodeId) {
    target.entries.remove(id);
    for group in target.groups.values_mut() {
        group.child_entry_ids.retain(|child| child != id);
    }
}

fn remove_group(target: &mut Database, id: &NodeId) -> (usize, usize) {
    let mut stack = vec![*id];
    let mut groups = Vec::new();
    while let Some(group_id) = stack.pop() {
        if let Some(group) = target.groups.get(&group_id) {
            stack.extend(group.child_group_ids.iter().copied());
            for entry_id in &group.child_entry_ids {
                target.entries.remove(entry_id);
            }
            groups.push((group_id, group.child_entry_ids.len()));
        }
    }
    for group in target.groups.values_mut() {
        group.child_group_ids.retain(|child| child != id);
    }
    let entry_count = groups.iter().map(|(_, count)| count).sum();
    for (group_id, _) in &groups {
        target.groups.remove(group_id);
    }
    (groups.len(), entry_count)
}

fn deleted_time(db: &Database, id: &NodeId) -> Option<i64> {
    db.deleted_objects
        .iter()
        .filter(|deleted| &deleted.id == id)
        .map(|deleted| deleted.deletion_time)
        .max()
}

fn clear_deleted(db: &mut Database, id: &NodeId) {
    db.deleted_objects.retain(|deleted| &deleted.id != id);
}

fn merge_deleted_objects(target: &mut Database, source: &Database) {
    for deleted in &source.deleted_objects {
        if let Some(existing) = target
            .deleted_objects
            .iter_mut()
            .find(|item| item.id == deleted.id)
        {
            if deleted.deletion_time > existing.deletion_time {
                *existing = deleted.clone();
            }
        } else {
            target.deleted_objects.push(deleted.clone());
        }
    }
}

fn merge_changed(result: &MergeResult) -> bool {
    result.entries_added
        + result.entries_modified
        + result.entries_deleted
        + result.groups_added
        + result.groups_modified
        + result.groups_deleted
        > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::core::security::ProtectedString;
    use crate::model::db::database::DatabaseVersion;
    use crate::model::meta::DeletedObject;

    fn database_with_root(root_id: NodeId) -> Database {
        let mut db = Database::new(DatabaseVersion::KDBX4);
        db.root_group_id = Some(root_id);
        db.groups.insert(root_id, Group::new(root_id));
        db
    }

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
            e.last_modification_time.as_millis().unwrap_or(0) + 100_000,
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
        target
            .entries
            .insert(entry_id, make_entry_with_title(entry_id, "Old"));
        source
            .entries
            .insert(entry_id, make_entry_newer(entry_id, "New"));

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
        target
            .entries
            .insert(entry_id, make_entry_with_title(entry_id, "Original"));
        source
            .entries
            .insert(entry_id, make_entry_newer(entry_id, "Modified"));

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
        let base_entry = make_entry_with_title(entry_id, "Base");
        base.entries.insert(entry_id, base_entry.clone());
        target.entries.insert(entry_id, base_entry.clone());
        let mut source_entry = base_entry;
        source_entry.title = "Source Modified".into();
        source_entry.last_modification_time = crate::model::core::date::DateInstant::EpochMillis(
            source_entry.last_modified() + 100_000,
        );
        source.entries.insert(entry_id, source_entry);

        let merger = DatabaseMerger::new(MergeStrategy::Overwrite);
        let result = merger.merge_three_way(&mut target, &source, &base);

        assert_eq!(result.conflicts.len(), 0);
        assert_eq!(
            target.entries.get(&entry_id).unwrap().title,
            "Source Modified"
        );
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
        source
            .entries
            .insert(new_id, make_entry_with_title(new_id, "New Entry"));

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

    #[test]
    fn three_way_password_only_change_conflicts() {
        let root_id = NodeId::new_uuid();
        let entry_id = NodeId::new_uuid();
        let mut base = database_with_root(root_id);
        let mut entry = Entry::new(entry_id);
        entry.password = ProtectedString::new_protected("base");
        base.add_entry(entry.clone(), &root_id);

        let mut target = database_with_root(root_id);
        let mut target_entry = entry.clone();
        target_entry.password = ProtectedString::new_protected("target");
        target.add_entry(target_entry, &root_id);

        let mut source = database_with_root(root_id);
        entry.password = ProtectedString::new_protected("source");
        source.add_entry(entry, &root_id);

        let result = DatabaseMerger::new(MergeStrategy::KeepExisting).merge_three_way(
            &mut target,
            &source,
            &base,
        );

        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(target.entries[&entry_id].password.as_str(), "target");
        target.validate().unwrap();
    }

    #[test]
    fn added_group_and_entry_are_attached_to_source_parent() {
        let root_id = NodeId::new_uuid();
        let group_id = NodeId::new_uuid();
        let entry_id = NodeId::new_uuid();
        let mut target = database_with_root(root_id);
        let mut source = database_with_root(root_id);
        source.add_group(Group::new(group_id), &root_id);
        source.add_entry(Entry::new(entry_id), &group_id);

        DatabaseMerger::new(MergeStrategy::Overwrite).merge(&mut target, &source);

        assert_eq!(target.find_parent_group_of_entry(&entry_id), Some(group_id));
        assert!(target.groups[&root_id].child_group_ids.contains(&group_id));
        target.validate().unwrap();
    }

    #[test]
    fn keep_both_duplicate_is_attached() {
        let root_id = NodeId::new_uuid();
        let entry_id = NodeId::new_uuid();
        let mut target = database_with_root(root_id);
        let original = make_entry_with_title(entry_id, "target");
        target.add_entry(original, &root_id);
        let mut source = database_with_root(root_id);
        source.add_entry(make_entry_newer(entry_id, "source"), &root_id);

        DatabaseMerger::new(MergeStrategy::KeepBoth).merge(&mut target, &source);

        assert_eq!(target.entries.len(), 2);
        assert_eq!(target.groups[&root_id].child_entry_ids.len(), 2);
        target.validate().unwrap();
    }

    #[test]
    fn source_deletion_uses_timestamp_and_cleans_parent_reference() {
        let root_id = NodeId::new_uuid();
        let entry_id = NodeId::new_uuid();
        let mut target = database_with_root(root_id);
        let mut entry = Entry::new(entry_id);
        entry.last_modification_time = crate::model::core::date::DateInstant::EpochMillis(100);
        target.add_entry(entry, &root_id);
        let mut source = database_with_root(root_id);
        source.deleted_objects.push(DeletedObject {
            id: entry_id,
            deletion_time: 99,
        });

        let old = DatabaseMerger::new(MergeStrategy::Overwrite).merge(&mut target, &source);
        assert!(target.entries.contains_key(&entry_id));
        assert_eq!(old.conflicts.len(), 1);

        source.deleted_objects[0].deletion_time = 101;
        let new = DatabaseMerger::new(MergeStrategy::Overwrite).merge(&mut target, &source);
        assert_eq!(new.entries_deleted, 1);
        assert!(!target.groups[&root_id].child_entry_ids.contains(&entry_id));
        target.validate().unwrap();
    }

    #[test]
    fn three_way_group_content_keeps_target_only_and_takes_source_only_change() {
        let root_id = NodeId::new_uuid();
        let group_id = NodeId::new_uuid();
        let mut base = database_with_root(root_id);
        let mut group = Group::new(group_id);
        group.title = "base".into();
        base.add_group(group.clone(), &root_id);

        let mut target = database_with_root(root_id);
        let mut target_group = group.clone();
        target_group.title = "target".into();
        target.add_group(target_group, &root_id);
        let mut source = database_with_root(root_id);
        source.add_group(group.clone(), &root_id);

        DatabaseMerger::new(MergeStrategy::Overwrite).merge_three_way(&mut target, &source, &base);
        assert_eq!(target.groups[&group_id].title, "target");

        let mut target = database_with_root(root_id);
        target.add_group(group.clone(), &root_id);
        let mut source = database_with_root(root_id);
        group.title = "source".into();
        source.add_group(group, &root_id);

        DatabaseMerger::new(MergeStrategy::Overwrite).merge_three_way(&mut target, &source, &base);
        assert_eq!(target.groups[&group_id].title, "source");
        target.validate().unwrap();
    }

    #[test]
    fn three_way_group_parent_keeps_target_only_and_takes_source_only_move() {
        let root_id = NodeId::new_uuid();
        let left_id = NodeId::new_uuid();
        let right_id = NodeId::new_uuid();
        let child_id = NodeId::new_uuid();
        let build = |child_parent: NodeId| {
            let mut db = database_with_root(root_id);
            db.add_group(Group::new(left_id), &root_id);
            db.add_group(Group::new(right_id), &root_id);
            db.add_group(Group::new(child_id), &child_parent);
            db
        };
        let base = build(left_id);
        let source = build(left_id);
        let mut target = build(right_id);

        DatabaseMerger::new(MergeStrategy::Overwrite).merge_three_way(&mut target, &source, &base);
        assert_eq!(group_parent(&target, &child_id), Some(right_id));
        target.validate().unwrap();

        let source = build(right_id);
        let mut target = build(left_id);
        DatabaseMerger::new(MergeStrategy::Overwrite).merge_three_way(&mut target, &source, &base);
        assert_eq!(group_parent(&target, &child_id), Some(right_id));
        target.validate().unwrap();
    }
}
