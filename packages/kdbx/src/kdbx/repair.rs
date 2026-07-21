//! Database integrity verification and recovery
//!
//! Provides tools for detecting and repairing corrupted databases.

use crate::model::core::node::NodeId;
use crate::model::db::database::Database;

/// Result of a database integrity check.
#[derive(Debug, Clone, Default)]
pub struct IntegrityReport {
    pub errors: Vec<IntegrityError>,
    pub warnings: Vec<IntegrityWarning>,
    pub orphan_entries: Vec<NodeId>,
    pub orphan_groups: Vec<NodeId>,
    pub duplicate_child_refs: usize,
    pub invalid_parent_refs: usize,
}

/// Types of integrity errors.
#[derive(Debug, Clone)]
pub enum IntegrityError {
    /// Entry referenced in a group but not in database.entries
    MissingEntry(NodeId),
    /// Group referenced as child but not in database.groups
    MissingGroup(NodeId),
    /// Entry exists but not referenced from any group
    OrphanEntry(NodeId),
    /// Group exists but not reachable from root
    OrphanGroup(NodeId),
    /// Root group is missing
    NoRootGroup,
    /// Group references itself as child
    SelfReference(NodeId),
}

/// Types of integrity warnings.
#[derive(Debug, Clone)]
pub enum IntegrityWarning {
    /// Duplicate child reference in a group
    DuplicateChildRef { group_id: NodeId, child_id: NodeId },
    /// Entry has empty title
    EmptyTitle(NodeId),
    /// Group has empty title
    EmptyGroupTitle(NodeId),
    /// Entry references a group that doesn't contain it
    MisplacedEntry {
        entry_id: NodeId,
        expected_group: NodeId,
    },
}

/// Database integrity verifier.
pub struct IntegrityVerifier;

impl IntegrityVerifier {
    /// Verify database integrity and return a report.
    pub fn verify(database: &Database) -> IntegrityReport {
        let mut report = IntegrityReport::default();

        // Check root group
        if database.root_group_id.is_none() {
            report.errors.push(IntegrityError::NoRootGroup);
            return report;
        }

        let root_id = database.root_group_id.expect("checked above");

        // Collect all reachable groups and entries
        let mut reachable_groups = std::collections::HashSet::new();
        let mut reachable_entries = std::collections::HashSet::new();
        Self::collect_reachable(
            database,
            &root_id,
            &mut reachable_groups,
            &mut reachable_entries,
            &mut report,
        );

        // Check for orphan entries (in database.entries but not reachable)
        for entry_id in database.entries.keys() {
            if !reachable_entries.contains(entry_id) {
                report.orphan_entries.push(*entry_id);
                report.errors.push(IntegrityError::OrphanEntry(*entry_id));
            }
        }

        // Check for orphan groups (in database.groups but not reachable from root)
        for group_id in database.groups.keys() {
            if !reachable_groups.contains(group_id) {
                report.orphan_groups.push(*group_id);
                report.errors.push(IntegrityError::OrphanGroup(*group_id));
            }
        }

        // Check for empty titles
        for (id, entry) in &database.entries {
            if entry.title().is_empty() && !entry.title().is_memory_protected() {
                report.warnings.push(IntegrityWarning::EmptyTitle(*id));
            }
        }
        for (id, group) in &database.groups {
            if group.title.is_empty() && *id != root_id {
                report.warnings.push(IntegrityWarning::EmptyGroupTitle(*id));
            }
        }

        report
    }

    /// Attempt to repair database integrity issues.
    /// Returns the number of fixes applied.
    pub fn repair(database: &mut Database) -> RepairResult {
        let report = Self::verify(database);
        let mut result = RepairResult::default();

        // 1. Remove orphan entries
        for orphan_id in &report.orphan_entries {
            database.entries.remove(orphan_id);
            result.entries_removed += 1;
        }

        // 2. Try to re-attach orphan groups to root
        if let Some(root_id) = database.root_group_id {
            for orphan_id in &report.orphan_groups {
                if let Some(_group) = database.groups.get_mut(orphan_id) {
                    // Attach to root
                    if let Some(root) = database.groups.get_mut(&root_id) {
                        root.add_child_group(*orphan_id);
                        result.groups_reattached += 1;
                    }
                }
            }
        }

        // 3. Fix missing entry refs in groups — remove refs to non-existent entries
        let all_entry_ids: std::collections::HashSet<NodeId> =
            database.entries.keys().copied().collect();
        let all_group_ids: std::collections::HashSet<NodeId> =
            database.groups.keys().copied().collect();

        for group in database.groups.values_mut() {
            let before = group.child_entry_ids.len();
            group
                .child_entry_ids
                .retain(|id| all_entry_ids.contains(id));
            result.invalid_refs_removed += before - group.child_entry_ids.len();

            let before = group.child_group_ids.len();
            group
                .child_group_ids
                .retain(|id| all_group_ids.contains(id));
            result.invalid_refs_removed += before - group.child_group_ids.len();
        }

        // 4. Remove self-references
        for (group_id, group) in database.groups.iter_mut() {
            let before = group.child_group_ids.len();
            group.child_group_ids.retain(|id| id != group_id);
            result.self_refs_removed += before - group.child_group_ids.len();
        }

        // 5. Deduplicate child references
        for group in database.groups.values_mut() {
            Self::deduplicate_vec(
                &mut group.child_group_ids,
                &mut result.duplicate_refs_removed,
            );
            Self::deduplicate_vec(
                &mut group.child_entry_ids,
                &mut result.duplicate_refs_removed,
            );
        }

        if result.total_fixes() > 0 {
            database.mark_modified();
        }

        result
    }

    fn collect_reachable(
        db: &Database,
        group_id: &NodeId,
        reachable_groups: &mut std::collections::HashSet<NodeId>,
        reachable_entries: &mut std::collections::HashSet<NodeId>,
        report: &mut IntegrityReport,
    ) {
        if reachable_groups.contains(group_id) {
            return; // Already visited — avoid cycles
        }
        reachable_groups.insert(*group_id);

        let Some(group) = db.groups.get(group_id) else {
            report.errors.push(IntegrityError::MissingGroup(*group_id));
            return;
        };

        // Check for self-reference
        if group.child_group_ids.contains(group_id) {
            report.errors.push(IntegrityError::SelfReference(*group_id));
        }

        // Check child groups
        for child_id in &group.child_group_ids {
            if !db.groups.contains_key(child_id) {
                report.errors.push(IntegrityError::MissingGroup(*child_id));
            } else {
                Self::collect_reachable(db, child_id, reachable_groups, reachable_entries, report);
            }
        }

        // Check child entries
        for entry_id in &group.child_entry_ids {
            if !db.entries.contains_key(entry_id) {
                report.errors.push(IntegrityError::MissingEntry(*entry_id));
            } else {
                reachable_entries.insert(*entry_id);
            }
        }
    }

    fn deduplicate_vec(vec: &mut Vec<NodeId>, count: &mut usize) {
        let mut seen = std::collections::HashSet::new();
        let before = vec.len();
        vec.retain(|id| seen.insert(*id));
        *count += before - vec.len();
    }
}

/// Result of a repair operation.
#[derive(Debug, Clone, Default)]
pub struct RepairResult {
    pub entries_removed: usize,
    pub groups_reattached: usize,
    pub invalid_refs_removed: usize,
    pub self_refs_removed: usize,
    pub duplicate_refs_removed: usize,
}

impl RepairResult {
    pub fn total_fixes(&self) -> usize {
        self.entries_removed
            + self.groups_reattached
            + self.invalid_refs_removed
            + self.self_refs_removed
            + self.duplicate_refs_removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::db::database::DatabaseVersion;
    use crate::model::entry::Entry;
    use crate::model::group::Group;

    fn make_healthy_db() -> Database {
        let mut db = Database::new(DatabaseVersion::KDBX4);
        let root_id = NodeId::new_uuid();
        let mut root = Group::new(root_id);
        root.title = "Root".to_string();

        let entry_id = NodeId::new_uuid();
        let mut entry = Entry::new(entry_id);
        entry.set_title("Test");

        root.add_child_entry(entry_id);
        db.entries.insert(entry_id, entry);
        db.groups.insert(root_id, root);
        db.root_group_id = Some(root_id);
        db
    }

    #[test]
    fn test_verify_healthy_database() {
        let db = make_healthy_db();
        let report = IntegrityVerifier::verify(&db);
        assert!(report.errors.is_empty(), "Healthy DB should have no errors");
        assert!(report.orphan_entries.is_empty());
        assert!(report.orphan_groups.is_empty());
    }

    #[test]
    fn test_verify_orphan_entry() {
        let mut db = make_healthy_db();
        let orphan_id = NodeId::new_uuid();
        let mut orphan = Entry::new(orphan_id);
        orphan.set_title("Orphan");
        db.entries.insert(orphan_id, orphan);

        let report = IntegrityVerifier::verify(&db);
        assert_eq!(report.orphan_entries.len(), 1);
        assert_eq!(report.orphan_entries[0], orphan_id);
    }

    #[test]
    fn test_verify_missing_entry_ref() {
        let mut db = make_healthy_db();
        let root_id = db.root_group_id.unwrap();
        let ghost_id = NodeId::new_uuid();

        // Add a reference to a non-existent entry
        if let Some(root) = db.groups.get_mut(&root_id) {
            root.add_child_entry(ghost_id);
        }

        let report = IntegrityVerifier::verify(&db);
        assert!(report
            .errors
            .iter()
            .any(|e| matches!(e, IntegrityError::MissingEntry(id) if *id == ghost_id)));
    }

    #[test]
    fn test_verify_self_reference() {
        let mut db = make_healthy_db();
        let root_id = db.root_group_id.unwrap();

        if let Some(root) = db.groups.get_mut(&root_id) {
            root.add_child_group(root_id);
        }

        let report = IntegrityVerifier::verify(&db);
        assert!(report
            .errors
            .iter()
            .any(|e| matches!(e, IntegrityError::SelfReference(_))));
    }

    #[test]
    fn test_repair_removes_orphans() {
        let mut db = make_healthy_db();
        let orphan_id = NodeId::new_uuid();
        db.entries.insert(orphan_id, Entry::new(orphan_id));

        let result = IntegrityVerifier::repair(&mut db);
        assert_eq!(result.entries_removed, 1);
        assert!(!db.entries.contains_key(&orphan_id));
    }

    #[test]
    fn test_repair_removes_invalid_refs() {
        let mut db = make_healthy_db();
        let root_id = db.root_group_id.unwrap();
        let ghost_entry = NodeId::new_uuid();
        let ghost_group = NodeId::new_uuid();

        if let Some(root) = db.groups.get_mut(&root_id) {
            root.add_child_entry(ghost_entry);
            root.add_child_group(ghost_group);
        }

        let result = IntegrityVerifier::repair(&mut db);
        assert_eq!(result.invalid_refs_removed, 2);

        let root = db.groups.get(&root_id).unwrap();
        assert!(!root.child_entry_ids.contains(&ghost_entry));
        assert!(!root.child_group_ids.contains(&ghost_group));
    }

    #[test]
    fn test_repair_reattaches_orphan_groups() {
        let mut db = make_healthy_db();
        let orphan_group_id = NodeId::new_uuid();
        let mut orphan = Group::new(orphan_group_id);
        orphan.title = "Orphan Group".to_string();
        db.groups.insert(orphan_group_id, orphan);

        let result = IntegrityVerifier::repair(&mut db);
        assert_eq!(result.groups_reattached, 1);

        let root_id = db.root_group_id.unwrap();
        let root = db.groups.get(&root_id).unwrap();
        assert!(root.child_group_ids.contains(&orphan_group_id));
    }

    #[test]
    fn test_verify_no_root_group() {
        let mut db = Database::new(DatabaseVersion::KDBX4);
        db.root_group_id = None;

        let report = IntegrityVerifier::verify(&db);
        assert!(report
            .errors
            .iter()
            .any(|e| matches!(e, IntegrityError::NoRootGroup)));
    }
}
