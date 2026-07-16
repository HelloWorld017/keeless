//! Database merge engine
//!
//! Supports two-way merge and three-way merge with common ancestor.

mod three_way;

#[cfg(test)]
mod tests;

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
