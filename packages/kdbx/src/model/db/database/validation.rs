use std::collections::{HashMap, HashSet};

use super::Database;
use crate::model::exception::{DatabaseError, DatabaseResult};

impl Database {
    /// Validate the node graph before serialization.
    pub fn validate(&self) -> DatabaseResult<()> {
        let root_id = self
            .root_group_id
            .ok_or_else(|| DatabaseError::InvalidFormat("Database has no root group".into()))?;
        if !self.groups.contains_key(&root_id) {
            return Err(DatabaseError::InvalidFormat(
                "Root group reference is dangling".into(),
            ));
        }

        for (key, group) in &self.groups {
            if key != &group.id {
                return Err(DatabaseError::InvalidFormat(
                    "Group ID does not match its map key".into(),
                ));
            }
        }
        for (key, entry) in &self.entries {
            if key != &entry.id {
                return Err(DatabaseError::InvalidFormat(
                    "Entry ID does not match its map key".into(),
                ));
            }
        }

        let mut group_parents = HashMap::new();
        let mut entry_parents = HashMap::new();
        for (parent_id, group) in &self.groups {
            for child_id in &group.child_group_ids {
                if !self.groups.contains_key(child_id) {
                    return Err(DatabaseError::InvalidFormat(
                        "Dangling child group reference".into(),
                    ));
                }
                if group_parents.insert(*child_id, *parent_id).is_some() {
                    return Err(DatabaseError::InvalidFormat(
                        "Group has duplicate parents".into(),
                    ));
                }
            }
            for entry_id in &group.child_entry_ids {
                if !self.entries.contains_key(entry_id) {
                    return Err(DatabaseError::InvalidFormat(
                        "Dangling child entry reference".into(),
                    ));
                }
                if entry_parents.insert(*entry_id, *parent_id).is_some() {
                    return Err(DatabaseError::InvalidFormat(
                        "Entry has duplicate parents".into(),
                    ));
                }
            }
        }
        if group_parents.contains_key(&root_id) {
            return Err(DatabaseError::InvalidFormat(
                "Root group has a parent or participates in a cycle".into(),
            ));
        }

        let mut reachable_groups = HashSet::new();
        let mut reachable_entries = HashSet::new();
        let mut stack = vec![root_id];
        while let Some(group_id) = stack.pop() {
            if !reachable_groups.insert(group_id) {
                return Err(DatabaseError::InvalidFormat(
                    "Cycle or duplicate group reference detected".into(),
                ));
            }
            let group = &self.groups[&group_id];
            stack.extend(group.child_group_ids.iter().copied());
            reachable_entries.extend(group.child_entry_ids.iter().copied());
        }
        if reachable_groups.len() != self.groups.len() {
            return Err(DatabaseError::InvalidFormat(
                "Groups are unreachable from the root or form a cycle".into(),
            ));
        }
        if reachable_entries.len() != self.entries.len() {
            return Err(DatabaseError::InvalidFormat(
                "Entries are unreachable from the root".into(),
            ));
        }
        Ok(())
    }
}
