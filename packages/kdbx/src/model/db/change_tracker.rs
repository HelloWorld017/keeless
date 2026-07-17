//! Incremental save / change tracking
//!
//! Tracks modifications to a database for efficient incremental saves.
//! Instead of re-serializing the entire database, only changed entries
//! are written.

use std::collections::{HashMap, HashSet};

use crate::crypto::memory_protection::{MemoryField, MemoryUnlockSession};
use crate::model::core::node::NodeId;
use crate::model::db::composite_key::CompositeKey;
use crate::model::db::database::Database;
use crate::model::entry::Entry;
use crate::model::exception::DatabaseResult;
use crate::model::group::Group;

/// Change type recorded by the tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    /// New node created
    Created,
    /// Existing node modified
    Modified,
    /// Node deleted
    Deleted,
    /// Node moved to a different parent
    Moved,
}

/// A single recorded change.
#[derive(Debug, Clone)]
pub struct ChangeRecord {
    pub node_id: NodeId,
    pub change_type: ChangeType,
    pub timestamp_ms: i64,
    /// For moves, the new parent group ID
    pub new_parent: Option<NodeId>,
}

/// Tracks database changes for incremental saving.
///
/// Usage:
/// ```ignore
/// let mut tracker = ChangeTracker::new();
/// tracker.mark_created(entry_id);
/// // ... make changes ...
/// let changes = tracker.drain_changes();
/// ```
#[derive(Debug, Clone)]
pub struct ChangeTracker {
    changes: Vec<ChangeRecord>,
    entry_hashes: HashMap<NodeId, u64>,
    group_hashes: HashMap<NodeId, u64>,
}

impl ChangeTracker {
    pub fn new() -> Self {
        Self {
            changes: Vec::new(),
            entry_hashes: HashMap::new(),
            group_hashes: HashMap::new(),
        }
    }

    /// Create a tracker with a baseline snapshot of the current database state.
    pub fn from_snapshot(database: &Database) -> Self {
        let mut tracker = Self::new();
        for (id, entry) in &database.entries {
            tracker.entry_hashes.insert(*id, Self::hash_entry(entry));
        }
        for (id, group) in &database.groups {
            tracker.group_hashes.insert(*id, Self::hash_group(group));
        }
        tracker
    }

    /// Create a snapshot while transiently unlocking protected entry strings.
    pub fn from_snapshot_with_credentials(
        database: &Database,
        composite_key: &CompositeKey,
    ) -> DatabaseResult<Self> {
        let mut tracker = Self::new();
        let mut unlock = database.memory_unlock(composite_key);
        for (id, entry) in &database.entries {
            tracker
                .entry_hashes
                .insert(*id, Self::hash_entry_with_memory(entry, &mut unlock)?);
        }
        for (id, group) in &database.groups {
            tracker.group_hashes.insert(*id, Self::hash_group(group));
        }
        Ok(tracker)
    }

    /// Mark a node as created.
    pub fn mark_created(&mut self, node_id: NodeId) {
        self.changes.push(ChangeRecord {
            node_id,
            change_type: ChangeType::Created,
            timestamp_ms: Self::now_ms(),
            new_parent: None,
        });
    }

    /// Mark a node as modified.
    pub fn mark_modified(&mut self, node_id: NodeId) {
        self.changes.push(ChangeRecord {
            node_id,
            change_type: ChangeType::Modified,
            timestamp_ms: Self::now_ms(),
            new_parent: None,
        });
    }

    /// Mark a node as deleted.
    pub fn mark_deleted(&mut self, node_id: NodeId) {
        self.changes.push(ChangeRecord {
            node_id,
            change_type: ChangeType::Deleted,
            timestamp_ms: Self::now_ms(),
            new_parent: None,
        });
    }

    /// Mark a node as moved to a new parent.
    pub fn mark_moved(&mut self, node_id: NodeId, new_parent: NodeId) {
        self.changes.push(ChangeRecord {
            node_id,
            change_type: ChangeType::Moved,
            timestamp_ms: Self::now_ms(),
            new_parent: Some(new_parent),
        });
    }

    /// Get all recorded changes.
    pub fn changes(&self) -> &[ChangeRecord] {
        &self.changes
    }

    /// Get the number of changes.
    pub fn change_count(&self) -> usize {
        self.changes.len()
    }

    /// Check if there are any changes.
    pub fn has_changes(&self) -> bool {
        !self.changes.is_empty()
    }

    /// Drain all changes, returning them.
    pub fn drain_changes(&mut self) -> Vec<ChangeRecord> {
        std::mem::take(&mut self.changes)
    }

    /// Clear all recorded changes.
    pub fn clear(&mut self) {
        self.changes.clear();
    }

    /// Diff the current database state against the snapshot.
    /// Returns changes detected by comparing hashes.
    pub fn diff_against_snapshot(&mut self, database: &Database) -> DiffResult {
        let mut result = DiffResult::default();

        // Check entries
        let current_entry_ids: HashSet<NodeId> = database.entries.keys().copied().collect();
        for (id, entry) in &database.entries {
            let current_hash = Self::hash_entry(entry);
            match self.entry_hashes.get(id) {
                Some(&snapshot_hash) => {
                    if current_hash != snapshot_hash {
                        result.modified_entries.push(*id);
                        result.total_changes += 1;
                    }
                }
                None => {
                    result.new_entries.push(*id);
                    result.total_changes += 1;
                }
            }
        }

        // Check for deleted entries
        for id in self.entry_hashes.keys() {
            if !current_entry_ids.contains(id) {
                result.deleted_entries.push(*id);
                result.total_changes += 1;
            }
        }

        // Check groups
        let current_group_ids: HashSet<NodeId> = database.groups.keys().copied().collect();
        for (id, group) in &database.groups {
            let current_hash = Self::hash_group(group);
            match self.group_hashes.get(id) {
                Some(&snapshot_hash) => {
                    if current_hash != snapshot_hash {
                        result.modified_groups.push(*id);
                        result.total_changes += 1;
                    }
                }
                None => {
                    result.new_groups.push(*id);
                    result.total_changes += 1;
                }
            }
        }

        // Check for deleted groups
        for id in self.group_hashes.keys() {
            if !current_group_ids.contains(id) {
                result.deleted_groups.push(*id);
                result.total_changes += 1;
            }
        }

        result
    }

    /// Diff a loaded database without exposing protected field plaintext.
    pub fn diff_against_snapshot_with_credentials(
        &mut self,
        database: &Database,
        composite_key: &CompositeKey,
    ) -> DatabaseResult<DiffResult> {
        let mut result = DiffResult::default();
        let mut unlock = database.memory_unlock(composite_key);
        let current_entry_ids: HashSet<NodeId> = database.entries.keys().copied().collect();
        for (id, entry) in &database.entries {
            let current_hash = Self::hash_entry_with_memory(entry, &mut unlock)?;
            match self.entry_hashes.get(id) {
                Some(snapshot_hash) if current_hash != *snapshot_hash => {
                    result.modified_entries.push(*id);
                    result.total_changes += 1;
                }
                None => {
                    result.new_entries.push(*id);
                    result.total_changes += 1;
                }
                _ => {}
            }
        }
        for id in self.entry_hashes.keys() {
            if !current_entry_ids.contains(id) {
                result.deleted_entries.push(*id);
                result.total_changes += 1;
            }
        }
        let current_group_ids: HashSet<NodeId> = database.groups.keys().copied().collect();
        for (id, group) in &database.groups {
            let current_hash = Self::hash_group(group);
            match self.group_hashes.get(id) {
                Some(snapshot_hash) if current_hash != *snapshot_hash => {
                    result.modified_groups.push(*id);
                    result.total_changes += 1;
                }
                None => {
                    result.new_groups.push(*id);
                    result.total_changes += 1;
                }
                _ => {}
            }
        }
        for id in self.group_hashes.keys() {
            if !current_group_ids.contains(id) {
                result.deleted_groups.push(*id);
                result.total_changes += 1;
            }
        }
        Ok(result)
    }

    /// Update the snapshot to the current database state.
    pub fn update_snapshot(&mut self, database: &Database) {
        self.entry_hashes.clear();
        self.group_hashes.clear();
        for (id, entry) in &database.entries {
            self.entry_hashes.insert(*id, Self::hash_entry(entry));
        }
        for (id, group) in &database.groups {
            self.group_hashes.insert(*id, Self::hash_group(group));
        }
    }

    fn hash_entry(entry: &Entry) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        entry.title.hash(&mut hasher);
        entry.username.as_str().hash(&mut hasher);
        entry.url.hash(&mut hasher);
        entry.notes.as_str().hash(&mut hasher);
        hasher.finish()
    }

    fn hash_entry_with_memory(
        entry: &Entry,
        unlock: &mut MemoryUnlockSession<'_>,
    ) -> DatabaseResult<u64> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        for field in [
            MemoryField::Title,
            MemoryField::UserName,
            MemoryField::Url,
            MemoryField::Notes,
            MemoryField::Password,
        ] {
            entry.with_memory_field(unlock, &field, |value| {
                value.hash(&mut hasher);
                Ok(())
            })?;
        }
        for custom in &entry.custom_fields {
            custom.name.hash(&mut hasher);
            entry.with_memory_field(
                unlock,
                &MemoryField::Custom(custom.name.clone()),
                |value| {
                    value.hash(&mut hasher);
                    Ok(())
                },
            )?;
        }
        Ok(hasher.finish())
    }

    fn hash_group(group: &Group) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        group.title.hash(&mut hasher);
        for id in &group.child_entry_ids {
            id.hash(&mut hasher);
        }
        for id in &group.child_group_ids {
            id.hash(&mut hasher);
        }
        hasher.finish()
    }

    fn now_ms() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }
}

impl Default for ChangeTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of diffing a database against a snapshot.
#[derive(Debug, Clone, Default)]
pub struct DiffResult {
    pub new_entries: Vec<NodeId>,
    pub modified_entries: Vec<NodeId>,
    pub deleted_entries: Vec<NodeId>,
    pub new_groups: Vec<NodeId>,
    pub modified_groups: Vec<NodeId>,
    pub deleted_groups: Vec<NodeId>,
    pub total_changes: usize,
}

impl DiffResult {
    pub fn has_changes(&self) -> bool {
        self.total_changes > 0
    }

    pub fn summary(&self) -> String {
        if self.total_changes == 0 {
            return "No changes".to_string();
        }

        let mut parts = Vec::new();
        if !self.new_entries.is_empty() {
            parts.push(format!("+{} entries", self.new_entries.len()));
        }
        if !self.modified_entries.is_empty() {
            parts.push(format!("~{} entries", self.modified_entries.len()));
        }
        if !self.deleted_entries.is_empty() {
            parts.push(format!("-{} entries", self.deleted_entries.len()));
        }
        if !self.new_groups.is_empty() {
            parts.push(format!("+{} groups", self.new_groups.len()));
        }
        if !self.modified_groups.is_empty() {
            parts.push(format!("~{} groups", self.modified_groups.len()));
        }
        if !self.deleted_groups.is_empty() {
            parts.push(format!("-{} groups", self.deleted_groups.len()));
        }
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::db::database::DatabaseVersion;

    #[test]
    fn test_tracker_mark_created() {
        let mut tracker = ChangeTracker::new();
        let id = NodeId::new_uuid();
        tracker.mark_created(id);

        assert_eq!(tracker.change_count(), 1);
        assert_eq!(tracker.changes()[0].change_type, ChangeType::Created);
        assert_eq!(tracker.changes()[0].node_id, id);
    }

    #[test]
    fn test_tracker_mark_modified() {
        let mut tracker = ChangeTracker::new();
        let id = NodeId::new_uuid();
        tracker.mark_modified(id);

        assert!(tracker.has_changes());
        assert_eq!(tracker.changes()[0].change_type, ChangeType::Modified);
    }

    #[test]
    fn test_tracker_mark_deleted() {
        let mut tracker = ChangeTracker::new();
        let id = NodeId::new_uuid();
        tracker.mark_deleted(id);

        assert_eq!(tracker.changes()[0].change_type, ChangeType::Deleted);
    }

    #[test]
    fn test_tracker_mark_moved() {
        let mut tracker = ChangeTracker::new();
        let id = NodeId::new_uuid();
        let parent = NodeId::new_uuid();
        tracker.mark_moved(id, parent);

        assert_eq!(tracker.changes()[0].change_type, ChangeType::Moved);
        assert_eq!(tracker.changes()[0].new_parent, Some(parent));
    }

    #[test]
    fn test_tracker_drain() {
        let mut tracker = ChangeTracker::new();
        tracker.mark_created(NodeId::new_uuid());
        tracker.mark_modified(NodeId::new_uuid());

        let changes = tracker.drain_changes();
        assert_eq!(changes.len(), 2);
        assert!(!tracker.has_changes());
    }

    #[test]
    fn test_tracker_clear() {
        let mut tracker = ChangeTracker::new();
        tracker.mark_created(NodeId::new_uuid());
        tracker.clear();
        assert!(!tracker.has_changes());
    }

    #[test]
    fn test_snapshot_diff_detects_new_entry() {
        let mut db = Database::new(DatabaseVersion::KDBX4);
        let root_id = NodeId::new_uuid();
        let mut root = Group::new(root_id);
        root.title = "Root".to_string();
        db.groups.insert(root_id, root);
        db.root_group_id = Some(root_id);

        let mut tracker = ChangeTracker::from_snapshot(&db);

        // Add a new entry
        let entry_id = NodeId::new_uuid();
        let mut entry = Entry::new(entry_id);
        entry.title = "New Entry".to_string();
        db.entries.insert(entry_id, entry);

        let diff = tracker.diff_against_snapshot(&db);
        assert!(diff.has_changes());
        assert_eq!(diff.new_entries.len(), 1);
        assert_eq!(diff.new_entries[0], entry_id);
    }

    #[test]
    fn test_snapshot_diff_detects_modified_entry() {
        let mut db = Database::new(DatabaseVersion::KDBX4);
        let root_id = NodeId::new_uuid();
        db.groups.insert(root_id, Group::new(root_id));
        db.root_group_id = Some(root_id);

        let entry_id = NodeId::new_uuid();
        let mut entry = Entry::new(entry_id);
        entry.title = "Original".to_string();
        db.entries.insert(entry_id, entry);

        let mut tracker = ChangeTracker::from_snapshot(&db);

        // Modify entry
        db.entries.get_mut(&entry_id).unwrap().title = "Modified".to_string();

        let diff = tracker.diff_against_snapshot(&db);
        assert_eq!(diff.modified_entries.len(), 1);
        assert_eq!(diff.modified_entries[0], entry_id);
    }

    #[test]
    fn test_snapshot_diff_detects_deleted_entry() {
        let mut db = Database::new(DatabaseVersion::KDBX4);
        let root_id = NodeId::new_uuid();
        db.groups.insert(root_id, Group::new(root_id));
        db.root_group_id = Some(root_id);

        let entry_id = NodeId::new_uuid();
        db.entries.insert(entry_id, Entry::new(entry_id));

        let mut tracker = ChangeTracker::from_snapshot(&db);

        // Delete entry
        db.entries.remove(&entry_id);

        let diff = tracker.diff_against_snapshot(&db);
        assert!(diff.deleted_entries.contains(&entry_id));
    }

    #[test]
    fn test_snapshot_no_changes() {
        let mut db = Database::new(DatabaseVersion::KDBX4);
        let root_id = NodeId::new_uuid();
        db.groups.insert(root_id, Group::new(root_id));
        db.root_group_id = Some(root_id);

        let entry_id = NodeId::new_uuid();
        db.entries.insert(entry_id, Entry::new(entry_id));

        let mut tracker = ChangeTracker::from_snapshot(&db);
        let diff = tracker.diff_against_snapshot(&db);

        assert!(!diff.has_changes());
        assert_eq!(diff.summary(), "No changes");
    }

    #[test]
    fn test_snapshot_update() {
        let mut db = Database::new(DatabaseVersion::KDBX4);
        let root_id = NodeId::new_uuid();
        db.groups.insert(root_id, Group::new(root_id));
        db.root_group_id = Some(root_id);

        let mut tracker = ChangeTracker::from_snapshot(&db);

        // Add entry
        let entry_id = NodeId::new_uuid();
        db.entries.insert(entry_id, Entry::new(entry_id));

        // Update snapshot
        tracker.update_snapshot(&db);

        // Diff should show no changes now
        let diff = tracker.diff_against_snapshot(&db);
        assert!(!diff.has_changes());
    }

    #[test]
    fn test_diff_summary() {
        let result = DiffResult {
            new_entries: vec![NodeId::new_uuid(), NodeId::new_uuid()],
            modified_entries: vec![NodeId::new_uuid()],
            deleted_groups: vec![NodeId::new_uuid()],
            total_changes: 4,
            ..DiffResult::default()
        };

        let summary = result.summary();
        assert!(summary.contains("+2 entries"));
        assert!(summary.contains("~1 entries"));
        assert!(summary.contains("-1 groups"));
    }
}
