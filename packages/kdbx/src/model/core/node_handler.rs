//! Node traversal handler
//!

use crate::model::core::node::NodeId;
use crate::model::group::Group;
use crate::model::entry::Entry;
use crate::model::db::database::Database;

/// Traversal order for node tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalOrder {
    /// Parent first, then children (pre-order)
    TopDown,
    /// Children first, then parent (post-order)
    BottomUp,
}

/// Handler callback for node traversal.
pub trait NodeHandler {
    /// Called for each group during traversal.
    /// Return false to skip children of this group.
    fn handle_group(&mut self, _group: &Group, _group_id: &NodeId) -> bool {
        true
    }

    /// Called for each entry during traversal.
    fn handle_entry(&mut self, _entry: &Entry, _entry_id: &NodeId) {}
}

/// Traverse all nodes in the database tree.
pub fn traverse(db: &Database, order: TraversalOrder, handler: &mut dyn NodeHandler) {
    if let Some(root_id) = db.root_group_id.as_ref() {
        traverse_group(db, root_id, order, handler);
    }
}

/// Traverse a specific group subtree.
pub fn traverse_group(db: &Database, group_id: &NodeId, order: TraversalOrder, handler: &mut dyn NodeHandler) {
    let Some(group) = db.groups.get(group_id) else {
        return;
    };

    // Clone child lists to avoid borrow issues
    let child_groups = group.child_group_ids.clone();
    let child_entries = group.child_entry_ids.clone();

    if order == TraversalOrder::TopDown {
        if !handler.handle_group(group, group_id) {
            return; // Skip children
        }
        // Process entries
        for entry_id in &child_entries {
            if let Some(entry) = db.entries.get(entry_id) {
                handler.handle_entry(entry, entry_id);
            }
        }
        // Process child groups
        for child_id in &child_groups {
            traverse_group(db, child_id, order, handler);
        }
    } else {
        // BottomUp: children first
        for child_id in &child_groups {
            traverse_group(db, child_id, order, handler);
        }
        for entry_id in &child_entries {
            if let Some(entry) = db.entries.get(entry_id) {
                handler.handle_entry(entry, entry_id);
            }
        }
        handler.handle_group(group, group_id);
    }
}

/// Collect all entry IDs in a subtree.
pub fn collect_entry_ids(db: &Database, root_group_id: &NodeId) -> Vec<NodeId> {
    struct Collector { ids: Vec<NodeId> }
    impl NodeHandler for Collector {
        fn handle_entry(&mut self, _entry: &Entry, entry_id: &NodeId) {
            self.ids.push(*entry_id);
        }
    }
    let mut c = Collector { ids: Vec::new() };
    traverse_group(db, root_group_id, TraversalOrder::TopDown, &mut c);
    c.ids
}

/// Count all entries in a subtree.
pub fn count_entries(db: &Database, root_group_id: &NodeId) -> usize {
    struct Counter { count: usize }
    impl NodeHandler for Counter {
        fn handle_entry(&mut self, _entry: &Entry, _entry_id: &NodeId) { self.count += 1; }
    }
    let mut c = Counter { count: 0 };
    traverse_group(db, root_group_id, TraversalOrder::TopDown, &mut c);
    c.count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::group::Group;
    use crate::model::entry::Entry;
    use crate::model::db::database::DatabaseVersion;

    fn make_test_db() -> Database {
        let mut db = Database::new(DatabaseVersion::KDBX4);
        let root_id = NodeId::new_uuid();
        let root = Group::new(root_id);
        db.groups.insert(root_id, root);
        db.root_group_id = Some(root_id);
        db
    }

    #[test]
    fn test_traverse_empty_db() {
        let db = Database::new(DatabaseVersion::KDBX4);
        struct Counter { count: usize }
        impl NodeHandler for Counter {
            fn handle_group(&mut self, _: &Group, _: &NodeId) -> bool { self.count += 1; true }
        }
        let mut counter = Counter { count: 0 };
        traverse(&db, TraversalOrder::TopDown, &mut counter);
        assert_eq!(counter.count, 0);
    }

    #[test]
    fn test_traverse_with_entries() {
        let mut db = make_test_db();
        let root_id = db.root_group_id.unwrap();

        let sub_id = NodeId::new_uuid();
        let mut sub = Group::new(sub_id);
        sub.title = "Sub".to_string();
        db.add_group(sub, &root_id);

        let e1 = Entry::new(NodeId::new_uuid());
        let e1_id = e1.id;
        db.add_entry(e1, &root_id);
        let e2 = Entry::new(NodeId::new_uuid());
        db.add_entry(e2, &sub_id);

        let ids = collect_entry_ids(&db, &root_id);
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&e1_id));
        assert_eq!(count_entries(&db, &root_id), 2);
    }

    #[test]
    fn test_traverse_skip_children() {
        let mut db = make_test_db();
        let root_id = db.root_group_id.unwrap();

        let sub_id = NodeId::new_uuid();
        let mut sub = Group::new(sub_id);
        sub.title = "Skip".to_string();
        db.add_group(sub, &root_id);

        let entry = Entry::new(NodeId::new_uuid());
        db.add_entry(entry, &sub_id);

        struct SkipGroup { groups: usize, entries: usize }
        impl NodeHandler for SkipGroup {
            fn handle_group(&mut self, g: &Group, _: &NodeId) -> bool {
                self.groups += 1;
                g.title != "Skip"
            }
            fn handle_entry(&mut self, _: &Entry, _: &NodeId) { self.entries += 1; }
        }

        let mut h = SkipGroup { groups: 0, entries: 0 };
        traverse(&db, TraversalOrder::TopDown, &mut h);
        assert_eq!(h.groups, 2);
        assert_eq!(h.entries, 0);
    }
}
