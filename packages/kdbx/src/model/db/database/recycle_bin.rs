use crate::model::core::node::NodeId;
use crate::model::group::Group;
use crate::model::meta::DeletedObject;

use super::Database;

impl Database {
    /// Delete an entry using recycle-bin semantics after validating the complete mutation.
    /// Returns `false` without modifying the database when the requested mode is invalid.
    pub fn delete_entry(&mut self, entry_id: &NodeId, permanent: bool) -> bool {
        let Some(parent_id) = self.find_parent_group_of_entry(entry_id) else {
            return false;
        };
        if !self.entries.contains_key(entry_id) {
            return false;
        }

        let in_recycle_bin = self.is_entry_in_recycle_bin(entry_id);
        if permanent != in_recycle_bin {
            return false;
        }

        if permanent {
            self.entries.remove(entry_id);
            if let Some(parent) = self.groups.get_mut(&parent_id) {
                parent.child_entry_ids.retain(|id| id != entry_id);
            }
            self.deleted_objects.push(DeletedObject::new(*entry_id));
            self.mark_modified();
            return true;
        }

        let has_recycle_bin = self
            .recycle_bin_uuid
            .map(NodeId::from_uuid)
            .is_some_and(|id| self.groups.contains_key(&id));
        if !has_recycle_bin
            && !self
                .root_group_id
                .is_some_and(|id| self.groups.contains_key(&id))
        {
            return false;
        }
        let recycle_id = self.create_recycle_bin();
        debug_assert!(self.groups.contains_key(&recycle_id));
        debug_assert_ne!(parent_id, recycle_id);
        self.reposition_entry(entry_id, &recycle_id)
    }

    /// Create a recycle bin group if one doesn't exist.
    pub fn create_recycle_bin(&mut self) -> NodeId {
        if let Some(uuid) = self.recycle_bin_uuid {
            let id = NodeId::from_uuid(uuid);
            if self.groups.contains_key(&id) {
                return id;
            }
        }

        let recycle_id = NodeId::new_uuid();
        let mut recycle_group = Group::new(recycle_id);
        recycle_group.title = "Recycle Bin".to_string();
        recycle_group.enable_searching = false;
        recycle_group.is_expanded = false;

        if let Some(root_id) = self.root_group_id {
            self.groups.insert(recycle_id, recycle_group);
            if let Some(root) = self.groups.get_mut(&root_id) {
                root.add_child_group(recycle_id);
            }
        }
        if let NodeId::Uuid(uuid) = recycle_id {
            self.recycle_bin_uuid = Some(uuid);
        }
        self.mark_modified();
        recycle_id
    }

    /// Empty the recycle bin, permanently deleting all contained items.
    pub fn empty_recycle_bin(&mut self) {
        let Some(recycle_id) = self.recycle_bin_uuid.map(NodeId::from_uuid) else {
            return;
        };
        let Some(recycle) = self.groups.get(&recycle_id) else {
            return;
        };
        let entries_to_delete = recycle.child_entry_ids.clone();
        let groups_to_delete = recycle.child_group_ids.clone();

        for entry_id in entries_to_delete {
            self.entries.remove(&entry_id);
            self.deleted_objects.push(DeletedObject::new(entry_id));
        }
        for group_id in groups_to_delete {
            for descendant_id in self.collect_descendant_groups(&group_id) {
                if let Some(group) = self.groups.get(&descendant_id) {
                    for entry_id in &group.child_entry_ids {
                        self.entries.remove(entry_id);
                        self.deleted_objects.push(DeletedObject::new(*entry_id));
                    }
                }
                self.groups.remove(&descendant_id);
                self.deleted_objects.push(DeletedObject::new(descendant_id));
            }
        }
        if let Some(recycle) = self.groups.get_mut(&recycle_id) {
            recycle.child_entry_ids.clear();
            recycle.child_group_ids.clear();
        }
        self.mark_modified();
    }

    /// Check if a group is the recycle bin.
    pub fn is_recycle_bin(&self, group_id: &NodeId) -> bool {
        self.recycle_bin_uuid
            .is_some_and(|uuid| *group_id == NodeId::from_uuid(uuid))
    }

    /// Returns whether an entry is directly or indirectly contained by the recycle bin.
    pub fn is_entry_in_recycle_bin(&self, entry_id: &NodeId) -> bool {
        let Some(recycle_id) = self.recycle_bin_uuid.map(NodeId::from_uuid) else {
            return false;
        };
        self.find_parent_group_of_entry(entry_id)
            .is_some_and(|parent| {
                self.collect_descendant_groups(&recycle_id)
                    .contains(&parent)
            })
    }
}
