use std::collections::HashSet;

use crate::model::core::date::DateInstant;
use crate::model::core::node::NodeId;
use crate::model::group::Group;
use crate::model::meta::icon::{IconImage, IconImageStandard};
use crate::model::meta::DeletedObject;

use super::{Database, IconUpdate};

impl Database {
    /// Immutable reference to the root group, always read from `self.groups`.
    ///
    /// Returns `None` if no root group has been set yet.
    pub fn root_group(&self) -> Option<&Group> {
        self.root_group_id
            .as_ref()
            .and_then(|id| self.groups.get(id))
    }

    /// Mutable reference to the root group, always read from `self.groups`.
    pub fn root_group_mut(&mut self) -> Option<&mut Group> {
        let id = self.root_group_id.as_ref()?;
        self.groups.get_mut(id)
    }

    /// Get a group by its ID.
    pub fn get_group(&self, id: &NodeId) -> Option<&Group> {
        self.groups.get(id)
    }

    /// Get a mutable group by its ID.
    pub fn get_group_mut(&mut self, id: &NodeId) -> Option<&mut Group> {
        self.groups.get_mut(id)
    }

    /// Get the total number of groups.
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// Add a group to the database under the specified parent group.
    pub fn add_group(&mut self, group: Group, parent_group_id: &NodeId) -> bool {
        if parent_group_id == &group.id {
            return false;
        }
        if !self.groups.contains_key(parent_group_id) && self.root_group_id.is_none() {
            // Allow adding the root group.
        } else if !self.groups.contains_key(parent_group_id) {
            return false;
        }
        let group_id = group.id;
        self.groups.insert(group_id, group);
        if let Some(parent) = self.groups.get_mut(parent_group_id) {
            parent.add_child_group(group_id);
        }
        if self.root_group_id.is_none() {
            self.root_group_id = Some(group_id);
        }
        self.mark_modified();
        true
    }

    /// Rename a group and update its modification metadata.
    pub fn rename_group(&mut self, group_id: &NodeId, name: String) -> bool {
        let Some(group) = self.groups.get_mut(group_id) else {
            return false;
        };
        group.title = name;
        group.last_modification_time = DateInstant::now();
        self.mark_modified();
        true
    }

    /// Atomically update a group's name and icon metadata.
    pub fn update_group(&mut self, group_id: &NodeId, name: String, icon: IconUpdate) -> bool {
        let Some(group) = self.groups.get_mut(group_id) else {
            return false;
        };
        let icon_image = IconImage::Standard(IconImageStandard::new(icon.standard_id));
        if group.title == name
            && group.icon == icon_image
            && group.custom_icon_uuid == icon.custom_uuid
        {
            return false;
        }
        group.title = name;
        group.icon = icon_image;
        group.custom_icon_uuid = icon.custom_uuid;
        group.last_modification_time = DateInstant::now();
        self.mark_modified();
        true
    }

    /// Remove a group and all its children. The root group cannot be removed.
    pub fn remove_group(&mut self, group_id: &NodeId, use_recycle_bin: bool) -> Option<Group> {
        if self.root_group_id == Some(*group_id) {
            return None;
        }

        if use_recycle_bin {
            if let Some(recycle_uuid) = self.recycle_bin_uuid {
                let recycle_id = NodeId::from_uuid(recycle_uuid);
                if recycle_id != *group_id && self.groups.contains_key(&recycle_id) {
                    return self.move_group(group_id, &recycle_id);
                }
            }
        }

        let parent_id = self.find_parent_group_of_group(group_id)?;
        let descendants = self.collect_descendant_groups(group_id);
        for descendant_id in &descendants {
            if let Some(group) = self.groups.get(descendant_id) {
                for entry_id in &group.child_entry_ids {
                    self.entries.remove(entry_id);
                    self.deleted_objects.push(DeletedObject::new(*entry_id));
                }
            }
        }
        for descendant_id in descendants.iter().skip(1) {
            self.groups.remove(descendant_id);
            self.deleted_objects
                .push(DeletedObject::new(*descendant_id));
        }
        if let Some(parent) = self.groups.get_mut(&parent_id) {
            parent.child_group_ids.retain(|id| id != group_id);
        }
        self.deleted_objects.push(DeletedObject::new(*group_id));
        self.mark_modified();
        self.groups.remove(group_id)
    }

    /// Move a group to a parent and place it at the requested final child index.
    /// Root and recycle-bin groups cannot be moved, and cycles are rejected.
    pub fn reposition_group(
        &mut self,
        group_id: &NodeId,
        new_parent_id: &NodeId,
        destination_index: usize,
    ) -> bool {
        if self.root_group_id == Some(*group_id)
            || self.is_recycle_bin(group_id)
            || self.is_recycle_bin(new_parent_id)
            || group_id == new_parent_id
            || !self.groups.contains_key(group_id)
            || !self.groups.contains_key(new_parent_id)
            || self
                .collect_descendant_groups(group_id)
                .contains(new_parent_id)
        {
            return false;
        }

        let Some(old_parent_id) = self.find_parent_group_of_group(group_id) else {
            return false;
        };
        let destination_len = self
            .groups
            .get(new_parent_id)
            .map(|parent| {
                parent.child_group_ids.len() - usize::from(old_parent_id == *new_parent_id)
            })
            .unwrap_or_default();
        if destination_index > destination_len {
            return false;
        }

        if let Some(parent) = self.groups.get_mut(&old_parent_id) {
            parent.child_group_ids.retain(|id| id != group_id);
        }
        if let Some(parent) = self.groups.get_mut(new_parent_id) {
            parent.child_group_ids.insert(destination_index, *group_id);
        }
        if let Some(group) = self.groups.get_mut(group_id) {
            group.location_changed = DateInstant::now();
        }
        self.mark_modified();
        true
    }

    pub(super) fn collect_descendant_groups(&self, group_id: &NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut stack = vec![*group_id];
        while let Some(id) = stack.pop() {
            if visited.insert(id) {
                result.push(id);
                if let Some(group) = self.groups.get(&id) {
                    stack.extend(group.child_group_ids.iter().rev().copied());
                }
            }
        }
        result
    }

    fn find_parent_group_of_group(&self, group_id: &NodeId) -> Option<NodeId> {
        for (parent_id, group) in &self.groups {
            if group.child_group_ids.contains(group_id) {
                return Some(*parent_id);
            }
        }
        None
    }

    fn move_group(&mut self, group_id: &NodeId, new_parent_id: &NodeId) -> Option<Group> {
        let old_parent_id = self.find_parent_group_of_group(group_id)?;
        if let Some(parent) = self.groups.get_mut(&old_parent_id) {
            parent.child_group_ids.retain(|id| id != group_id);
        }
        if let Some(parent) = self.groups.get_mut(new_parent_id) {
            parent.add_child_group(*group_id);
        }
        self.mark_modified();
        self.groups.get(group_id).cloned()
    }
}
