//! Database merge engine
//!
//! Supports two-way merge and three-way merge with common ancestor.

use std::cell::RefCell;

mod three_way;
mod two_way;

#[cfg(test)]
mod tests;

use crate::crypto::memory_protection::MemoryUnlockSession;
use crate::model::core::node::{Node, NodeId};
use crate::model::db::composite_key::CompositeKey;
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
pub struct DatabaseMerger<'a> {
    strategy: MergeStrategy,
    composite_key: Option<&'a CompositeKey>,
    memory_unlock: Option<RefCell<MemoryUnlockSession<'a>>>,
}

impl DatabaseMerger<'static> {
    pub fn new(strategy: MergeStrategy) -> Self {
        Self {
            strategy,
            composite_key: None,
            memory_unlock: None,
        }
    }
}

impl<'a> DatabaseMerger<'a> {
    pub fn with_credentials(strategy: MergeStrategy, composite_key: &'a CompositeKey) -> Self {
        Self {
            strategy,
            composite_key: Some(composite_key),
            memory_unlock: Some(RefCell::new(MemoryUnlockSession::new(composite_key))),
        }
    }

    fn entry_differs(&self, _a_db: &Database, a: &Entry, _b_db: &Database, b: &Entry) -> bool {
        let Some(_) = self.composite_key else {
            return entry_differs(a, b);
        };
        let Some(unlock) = &self.memory_unlock else {
            return true;
        };
        let mut unlock = unlock.borrow_mut();
        let Ok(mut a) = a.semantic_clone(&mut unlock) else {
            return true;
        };
        let Ok(mut b) = b.semantic_clone(&mut unlock) else {
            return true;
        };
        a.id = b.id;
        a.history.clear();
        a.xml_extensions.history.clear();
        b.history.clear();
        b.xml_extensions.history.clear();
        a != b
    }

    fn entry_history_differs(
        &self,
        _a_db: &Database,
        a: &Entry,
        _b_db: &Database,
        b: &Entry,
    ) -> bool {
        let Some(_) = self.composite_key else {
            return entry_history_differs(a, b);
        };
        let Some(unlock) = &self.memory_unlock else {
            return true;
        };
        let mut unlock = unlock.borrow_mut();
        match (a.semantic_clone(&mut unlock), b.semantic_clone(&mut unlock)) {
            (Ok(a), Ok(b)) => {
                a.history != b.history || a.xml_extensions.history != b.xml_extensions.history
            }
            _ => true,
        }
    }

    fn rebind_entry(&self, entry: &mut Entry, new_id: NodeId) -> bool {
        let Some(unlock) = &self.memory_unlock else {
            entry.id = new_id;
            for history in &mut entry.history {
                history.id = new_id;
            }
            return true;
        };
        entry
            .rebind_memory_protection(&mut unlock.borrow_mut(), new_id)
            .is_ok()
    }
}

/// Check if two entries have different content.
fn entry_differs(a: &Entry, b: &Entry) -> bool {
    entry_snapshot(a, a.id) != entry_snapshot(b, a.id)
}

fn entry_history_differs(a: &Entry, b: &Entry) -> bool {
    a.history != b.history || a.xml_extensions.history != b.xml_extensions.history
}

fn entry_snapshot(entry: &Entry, id: NodeId) -> Entry {
    let mut snapshot = entry.clone();
    snapshot.id = id;
    snapshot.history.clear();
    snapshot.xml_extensions.history.clear();
    snapshot
}

fn merge_entry_histories(mut winner: Entry, sources: &[&Entry]) -> Entry {
    let winner_id = winner.id;
    let winner_current = entry_snapshot(&winner, winner_id);
    let mut history = Vec::new();

    collect_history(&mut history, &winner.history, winner_id, &winner_current);
    for source in sources {
        collect_history(&mut history, &source.history, winner_id, &winner_current);
        push_history_snapshot(&mut history, source, winner_id, &winner_current);

        for extension in &source.xml_extensions.history {
            if !winner.xml_extensions.history.contains(extension) {
                winner.xml_extensions.history.push(extension.clone());
            }
        }
    }

    history.sort_by_key(Node::last_modified);
    winner.history = history;
    winner
}

fn collect_history(merged: &mut Vec<Entry>, entries: &[Entry], id: NodeId, winner_current: &Entry) {
    for entry in entries {
        collect_history(merged, &entry.history, id, winner_current);
        push_history_snapshot(merged, entry, id, winner_current);
    }
}

fn push_history_snapshot(
    merged: &mut Vec<Entry>,
    entry: &Entry,
    id: NodeId,
    winner_current: &Entry,
) {
    let snapshot = entry_snapshot(entry, id);
    if snapshot != *winner_current && !merged.contains(&snapshot) {
        merged.push(snapshot);
    }
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
    if group_parent(target, &id) == Some(parent) {
        return false;
    }
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

fn move_group_from(target: &mut Database, source: &Database, id: NodeId) -> bool {
    attach_group_from(target, source, id)
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

fn move_entry_from(target: &mut Database, source: &Database, id: NodeId) -> bool {
    let Some(source_parent) = source.find_parent_group_of_entry(&id) else {
        return false;
    };
    let Some(parent) = source_parent_in_target(target, source_parent) else {
        return false;
    };
    if !target.groups.contains_key(&parent) {
        return false;
    }
    if target.find_parent_group_of_entry(&id) == Some(parent) {
        return false;
    }
    for group in target.groups.values_mut() {
        group.child_entry_ids.retain(|child| child != &id);
    }
    target
        .groups
        .get_mut(&parent)
        .expect("parent group exists")
        .add_child_entry(id);
    true
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

fn merge_deleted_objects(target: &mut Database, source: &Database) -> bool {
    let mut changed = false;
    for deleted in &source.deleted_objects {
        if let Some(existing) = target
            .deleted_objects
            .iter_mut()
            .find(|item| item.id == deleted.id)
        {
            if deleted.deletion_time > existing.deletion_time {
                *existing = deleted.clone();
                changed = true;
            }
        } else {
            target.deleted_objects.push(deleted.clone());
            changed = true;
        }
    }
    changed
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
