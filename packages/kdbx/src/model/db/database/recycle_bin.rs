use crate::model::core::date::DateInstant;
use crate::model::core::node::NodeId;
use crate::model::group::Group;
use crate::model::meta::DeletedObject;
use uuid::Uuid;

use super::Database;

impl Database {
    /// Delete an entry using recycle-bin semantics after validating the complete mutation.
    /// Returns `false` without modifying the database when the requested mode is invalid.
    pub fn delete_entry(&mut self, entry_id: &NodeId, permanent: bool) -> bool {
        self.delete_entry_at(
            entry_id,
            permanent,
            Uuid::new_v4(),
            chrono::Utc::now().timestamp_millis(),
        )
    }

    /// Delete an entry with deterministic recycle-bin identity and timestamps.
    pub fn delete_entry_at(
        &mut self,
        entry_id: &NodeId,
        permanent: bool,
        recycle_bin_uuid: Uuid,
        timestamp_ms: i64,
    ) -> bool {
        if !self.can_delete_entry(entry_id, permanent, recycle_bin_uuid) {
            return false;
        }
        let Some(parent_id) = self.find_parent_group_of_entry(entry_id) else {
            return false;
        };

        if permanent {
            self.entries.remove(entry_id);
            if let Some(parent) = self.groups.get_mut(&parent_id) {
                parent.child_entry_ids.retain(|id| id != entry_id);
            }
            self.deleted_objects
                .push(DeletedObject::new_at(*entry_id, timestamp_ms));
            self.mark_modified();
            return true;
        }

        let Some(recycle_id) =
            self.create_recycle_bin_at(recycle_bin_uuid, DateInstant::EpochMillis(timestamp_ms))
        else {
            return false;
        };
        debug_assert!(self.groups.contains_key(&recycle_id));
        debug_assert_ne!(parent_id, recycle_id);
        self.reposition_entry_at(
            entry_id,
            &recycle_id,
            DateInstant::EpochMillis(timestamp_ms),
        )
    }

    /// Pure validation for deterministic entry deletion.
    pub fn can_delete_entry(
        &self,
        entry_id: &NodeId,
        permanent: bool,
        recycle_bin_uuid: Uuid,
    ) -> bool {
        self.entries.contains_key(entry_id)
            && self.find_parent_group_of_entry(entry_id).is_some()
            && permanent == self.is_entry_in_recycle_bin(entry_id)
            && (permanent || self.can_create_recycle_bin(recycle_bin_uuid))
    }

    /// Create a recycle bin group if one doesn't exist.
    pub fn create_recycle_bin(&mut self) -> NodeId {
        self.create_recycle_bin_at(Uuid::new_v4(), DateInstant::now())
            .expect("a database with a root group can create a recycle bin")
    }

    /// Create a recycle bin with caller-supplied identity and timestamps.
    /// Returns `None` without mutation when no valid root exists or the UUID collides.
    pub fn create_recycle_bin_at(
        &mut self,
        recycle_bin_uuid: Uuid,
        timestamp: DateInstant,
    ) -> Option<NodeId> {
        if let Some(uuid) = self.recycle_bin_uuid {
            let id = NodeId::from_uuid(uuid);
            if self.groups.contains_key(&id) {
                return Some(id);
            }
        }

        let root_id = self
            .root_group_id
            .filter(|root_id| self.groups.contains_key(root_id))?;
        let recycle_id = NodeId::from_uuid(recycle_bin_uuid);
        if self.groups.contains_key(&recycle_id) {
            return None;
        }
        let mut recycle_group = Group::new_at(recycle_id, timestamp);
        recycle_group.title = "Recycle Bin".to_string();
        recycle_group.enable_searching = false;
        recycle_group.is_expanded = false;

        self.groups.insert(recycle_id, recycle_group);
        self.groups
            .get_mut(&root_id)
            .expect("validated root group")
            .add_child_group(recycle_id);
        self.recycle_bin_uuid = Some(recycle_bin_uuid);
        self.mark_modified();
        Some(recycle_id)
    }

    /// Pure validation for deterministic recycle-bin creation.
    pub fn can_create_recycle_bin(&self, recycle_bin_uuid: Uuid) -> bool {
        if let Some(id) = self.recycle_bin_uuid.map(NodeId::from_uuid) {
            if self.groups.contains_key(&id) {
                return true;
            }
        }
        self.root_group_id
            .is_some_and(|root_id| self.groups.contains_key(&root_id))
            && !self
                .groups
                .contains_key(&NodeId::from_uuid(recycle_bin_uuid))
    }

    /// Delete a group using recycle-bin semantics.
    pub fn delete_group(&mut self, group_id: &NodeId, permanent: bool) -> bool {
        self.delete_group_at(
            group_id,
            permanent,
            Uuid::new_v4(),
            chrono::Utc::now().timestamp_millis(),
        )
    }

    /// Delete or recycle a group with deterministic identity and timestamps.
    pub fn delete_group_at(
        &mut self,
        group_id: &NodeId,
        permanent: bool,
        recycle_bin_uuid: Uuid,
        timestamp_ms: i64,
    ) -> bool {
        if !self.can_delete_group(group_id, permanent, recycle_bin_uuid) {
            return false;
        }
        let Some(parent_id) = self.find_parent_group_of_group(group_id) else {
            return false;
        };

        if !permanent {
            let Some(recycle_id) = self
                .create_recycle_bin_at(recycle_bin_uuid, DateInstant::EpochMillis(timestamp_ms))
            else {
                return false;
            };
            self.groups
                .get_mut(&parent_id)
                .expect("validated parent group")
                .child_group_ids
                .retain(|id| id != group_id);
            self.groups
                .get_mut(&recycle_id)
                .expect("prepared recycle bin")
                .add_child_group(*group_id);
            self.groups
                .get_mut(group_id)
                .expect("validated group")
                .location_changed = DateInstant::EpochMillis(timestamp_ms);
            self.mark_modified();
            return true;
        }

        let descendants = self.collect_descendant_groups(group_id);
        for descendant_id in &descendants {
            if let Some(group) = self.groups.get(descendant_id) {
                for entry_id in &group.child_entry_ids {
                    self.entries.remove(entry_id);
                    self.deleted_objects
                        .push(DeletedObject::new_at(*entry_id, timestamp_ms));
                }
            }
        }
        for descendant_id in descendants.iter().rev() {
            self.groups.remove(descendant_id);
            self.deleted_objects
                .push(DeletedObject::new_at(*descendant_id, timestamp_ms));
        }
        self.groups
            .get_mut(&parent_id)
            .expect("validated parent group")
            .child_group_ids
            .retain(|id| id != group_id);
        self.mark_modified();
        true
    }

    /// Pure validation for deterministic group deletion.
    pub fn can_delete_group(
        &self,
        group_id: &NodeId,
        permanent: bool,
        recycle_bin_uuid: Uuid,
    ) -> bool {
        self.root_group_id != Some(*group_id)
            && self.groups.contains_key(group_id)
            && self.find_parent_group_of_group(group_id).is_some()
            && permanent == self.is_group_in_recycle_bin(group_id)
            && (permanent || self.can_create_recycle_bin(recycle_bin_uuid))
    }

    /// Empty the recycle bin, permanently deleting all contained items.
    pub fn empty_recycle_bin(&mut self) {
        self.empty_recycle_bin_at(chrono::Utc::now().timestamp_millis());
    }

    /// Empty the recycle bin with a caller-supplied deletion timestamp.
    pub fn empty_recycle_bin_at(&mut self, timestamp_ms: i64) {
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
            self.deleted_objects
                .push(DeletedObject::new_at(entry_id, timestamp_ms));
        }
        for group_id in groups_to_delete {
            for descendant_id in self.collect_descendant_groups(&group_id) {
                if let Some(group) = self.groups.get(&descendant_id) {
                    for entry_id in &group.child_entry_ids {
                        self.entries.remove(entry_id);
                        self.deleted_objects
                            .push(DeletedObject::new_at(*entry_id, timestamp_ms));
                    }
                }
                self.groups.remove(&descendant_id);
                self.deleted_objects
                    .push(DeletedObject::new_at(descendant_id, timestamp_ms));
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

    /// Returns whether a group is directly or indirectly contained by the recycle bin.
    pub fn is_group_in_recycle_bin(&self, group_id: &NodeId) -> bool {
        let Some(recycle_id) = self.recycle_bin_uuid.map(NodeId::from_uuid) else {
            return false;
        };
        *group_id != recycle_id
            && self
                .collect_descendant_groups(&recycle_id)
                .contains(group_id)
    }
}
