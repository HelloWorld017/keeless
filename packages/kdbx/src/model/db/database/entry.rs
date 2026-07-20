use crate::crypto::memory_protection::{MemoryField, MemoryUnlockSession};
use crate::model::core::date::DateInstant;
use crate::model::core::node::NodeId;
use crate::model::db::composite_key::CompositeKey;
use crate::model::entry::Entry;
use crate::model::exception::{DatabaseError, DatabaseResult};
use crate::model::meta::DeletedObject;

use super::Database;

/// Selects an entry string for credential-scoped access or replacement.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EntryFieldSelector {
    Title,
    UserName,
    Password,
    Url,
    Notes,
    Custom(String),
}

impl EntryFieldSelector {
    fn memory_field(&self) -> MemoryField {
        match self {
            Self::Title => MemoryField::Title,
            Self::UserName => MemoryField::UserName,
            Self::Password => MemoryField::Password,
            Self::Url => MemoryField::Url,
            Self::Notes => MemoryField::Notes,
            Self::Custom(name) => MemoryField::Custom(name.clone()),
        }
    }
}

impl Database {
    /// Temporarily expose one entry field while the supplied credential is valid.
    pub fn with_entry_field<T>(
        &self,
        composite_key: &CompositeKey,
        entry_id: &NodeId,
        selector: &EntryFieldSelector,
        use_value: impl FnOnce(&str) -> T,
    ) -> DatabaseResult<T> {
        let entry = self
            .entries
            .get(entry_id)
            .ok_or_else(|| DatabaseError::InvalidFormat("entry does not exist".into()))?;
        let mut unlock = MemoryUnlockSession::new(composite_key);
        entry.with_memory_field(&mut unlock, &selector.memory_field(), |value| {
            Ok(use_value(value))
        })
    }

    /// Temporarily expose one custom field selected by its vector index.
    pub fn with_entry_custom_field<T>(
        &self,
        composite_key: &CompositeKey,
        entry_id: &NodeId,
        field_index: usize,
        use_value: impl FnOnce(&str) -> T,
    ) -> DatabaseResult<T> {
        let entry = self
            .entries
            .get(entry_id)
            .ok_or_else(|| DatabaseError::InvalidFormat("entry does not exist".into()))?;
        let field = entry
            .custom_fields
            .get(field_index)
            .ok_or_else(|| DatabaseError::InvalidFormat("unknown field index".into()))?;
        let memory_field = MemoryField::Custom(field.name.clone());
        let mut unlock = MemoryUnlockSession::new(composite_key);
        field
            .value
            .with_plaintext(&mut unlock, entry.id, &memory_field, |value| {
                Ok(use_value(value))
            })
    }

    /// Replace an entry field and immediately memory-protect it when requested.
    pub fn set_entry_field(
        &mut self,
        composite_key: &CompositeKey,
        entry_id: &NodeId,
        selector: &EntryFieldSelector,
        value: &str,
        protected: bool,
    ) -> DatabaseResult<()> {
        let context = match &self.memory_protection_context {
            Some(context) => context.clone(),
            None => {
                let (context, _) = self.create_memory_context(composite_key)?;
                self.memory_protection_context = Some(context.clone());
                context
            }
        };
        let mut unlock = MemoryUnlockSession::new(composite_key);
        unlock.with_root(&context, |root| {
            self.entries
                .get_mut(entry_id)
                .ok_or_else(|| DatabaseError::InvalidFormat("entry does not exist".into()))?
                .replace_memory_field(
                    context.clone(),
                    root,
                    &selector.memory_field(),
                    value,
                    protected,
                )
        })
    }

    /// Get an entry by its ID.
    pub fn get_entry(&self, id: &NodeId) -> Option<&Entry> {
        self.entries.get(id)
    }

    /// Get a mutable entry by its ID.
    pub fn get_entry_mut(&mut self, id: &NodeId) -> Option<&mut Entry> {
        self.entries.get_mut(id)
    }

    /// Get the total number of entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Add an entry to the database under the specified parent group.
    /// Updates both the entries map and the parent's child list.
    pub fn add_entry(&mut self, entry: Entry, parent_group_id: &NodeId) -> bool {
        if !self.groups.contains_key(parent_group_id) {
            return false;
        }
        let entry_id = entry.id;
        self.entries.insert(entry_id, entry);
        if let Some(parent) = self.groups.get_mut(parent_group_id) {
            parent.add_child_entry(entry_id);
        }
        self.mark_modified();
        true
    }

    /// Duplicate an entry with a fresh identity, redacting protected content when credentials
    /// are unavailable.
    pub fn duplicate_entry(
        &mut self,
        source_entry_id: &NodeId,
        parent_group_id: &NodeId,
        composite_key: Option<&CompositeKey>,
    ) -> DatabaseResult<Option<NodeId>> {
        if !self.groups.contains_key(parent_group_id) {
            return Ok(None);
        }
        let Some(mut entry) = self.entries.get(source_entry_id).cloned() else {
            return Ok(None);
        };

        let id = NodeId::new_uuid();
        let mut unlock = composite_key.map(MemoryUnlockSession::new);
        entry.prepare_duplicate(id, unlock.as_mut())?;
        Ok(self.add_entry(entry, parent_group_id).then_some(id))
    }

    /// Remove an entry from the database.
    /// If `use_recycle_bin` is true and a recycle bin exists, move to recycle bin instead.
    /// Returns the removed entry, or None if not found.
    pub fn remove_entry(&mut self, entry_id: &NodeId, use_recycle_bin: bool) -> Option<Entry> {
        let parent_id = self.find_parent_group_of_entry(entry_id)?;

        if use_recycle_bin {
            if let Some(ref recycle_uuid) = self.recycle_bin_uuid {
                let recycle_id = NodeId::from_uuid(*recycle_uuid);
                if recycle_id != *entry_id && self.groups.contains_key(&recycle_id) {
                    return self.move_entry(entry_id, &recycle_id);
                }
            }
        }

        let entry = self.entries.remove(entry_id)?;
        if let Some(parent) = self.groups.get_mut(&parent_id) {
            parent.child_entry_ids.retain(|id| id != entry_id);
        }
        self.deleted_objects.push(DeletedObject::new(*entry_id));
        self.mark_modified();
        Some(entry)
    }

    /// Move an entry to another group, appending it after existing entries.
    pub fn reposition_entry(&mut self, entry_id: &NodeId, new_parent_id: &NodeId) -> bool {
        if !self.entries.contains_key(entry_id) || !self.groups.contains_key(new_parent_id) {
            return false;
        }
        let Some(old_parent_id) = self.find_parent_group_of_entry(entry_id) else {
            return false;
        };
        if old_parent_id == *new_parent_id {
            return true;
        }

        if let Some(parent) = self.groups.get_mut(&old_parent_id) {
            parent.child_entry_ids.retain(|id| id != entry_id);
        }
        if let Some(parent) = self.groups.get_mut(new_parent_id) {
            parent.add_child_entry(*entry_id);
        }
        if let Some(entry) = self.entries.get_mut(entry_id) {
            entry.location_changed = DateInstant::now();
        }
        self.mark_modified();
        true
    }

    /// Find the parent group of an entry.
    pub fn find_parent_group_of_entry(&self, entry_id: &NodeId) -> Option<NodeId> {
        for (group_id, group) in &self.groups {
            if group.child_entry_ids.contains(entry_id) {
                return Some(*group_id);
            }
        }
        None
    }

    /// Get all entries in a group (non-recursive).
    pub fn get_entries_in_group(&self, group_id: &NodeId) -> Vec<&Entry> {
        if let Some(group) = self.groups.get(group_id) {
            group
                .child_entry_ids
                .iter()
                .filter_map(|id| self.entries.get(id))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get all entries in a group and its subgroups (recursive).
    pub fn get_all_entries_in_group(&self, group_id: &NodeId) -> Vec<&Entry> {
        let mut result = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut stack = vec![*group_id];
        while let Some(id) = stack.pop() {
            if visited.insert(id) {
                if let Some(group) = self.groups.get(&id) {
                    result.extend(
                        group
                            .child_entry_ids
                            .iter()
                            .filter_map(|entry_id| self.entries.get(entry_id)),
                    );
                    stack.extend(group.child_group_ids.iter().rev().copied());
                }
            }
        }
        result
    }

    fn move_entry(&mut self, entry_id: &NodeId, new_parent_id: &NodeId) -> Option<Entry> {
        self.reposition_entry(entry_id, new_parent_id)
            .then(|| self.entries.get(entry_id).cloned())
            .flatten()
    }
}
