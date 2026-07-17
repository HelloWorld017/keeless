//! Database root structure
//!

mod validation;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::crypto::compression::CompressionAlgorithm;
use crate::crypto::encryption_algorithm::EncryptionAlgorithm;
use crate::kdbx::kdf::kdf_parameters::KdfParameters;
use crate::model::core::node::NodeId;
use crate::model::core::security::MemoryProtectionConfig;
use crate::model::entry::Entry;
use crate::model::group::Group;
use crate::model::meta::icon::IconImageCustom;
use crate::model::meta::{CustomData, DeletedObject};
use crate::model::xml::DatabaseXmlExtensions;
/// Database version
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseVersion {
    /// KDB format (KeePass 1.x)
    KDB,
    /// KDBX 3.1 (KeePass 2.x pre-4)
    KDBX31,
    /// KDBX 4.0 (KeePass 2.x post-4)
    KDBX4,
}

/// The main KeePass database structure.
#[derive(Debug, Clone)]
pub struct Database {
    /// Database version
    pub version: DatabaseVersion,
    /// Exact on-disk format version, including the KDBX minor version.
    pub file_version: u32,
    /// Root group ID.
    ///
    /// **Single source of truth**: the root group is always looked up in
    /// `self.groups` via [`root_group`]`/`[`root_group_mut`]. We deliberately
    /// do NOT cache the root `Group` value here — a stale cached copy was the
    /// root cause of "added entries/groups disappear after save & reopen",
    /// because mutators updated `self.groups` but readers/writers read the
    /// stale cache.
    pub root_group_id: Option<NodeId>,
    /// All groups indexed by ID
    pub groups: HashMap<NodeId, Group>,
    /// All entries indexed by ID
    pub entries: HashMap<NodeId, Entry>,
    /// Deleted objects (KDBX 4.0 recycle bin)
    pub deleted_objects: Vec<DeletedObject>,
    /// Custom icons
    pub custom_icons: HashMap<Uuid, IconImageCustom>,
    /// Encryption algorithm
    pub encryption_algorithm: EncryptionAlgorithm,
    /// Compression algorithm
    pub compression: CompressionAlgorithm,
    /// KDF parameters
    pub kdf_parameters: Option<KdfParameters>,
    /// Raw KDBX4 public custom-data variant dictionary.
    pub public_custom_data: Vec<u8>,
    /// Optional KDBX4 outer-header comment.
    pub header_comment: Option<Vec<u8>>,
    /// Master key hash for verification
    pub master_key_hash: Option<Vec<u8>>,
    /// Database name
    pub name: String,
    /// Database description
    pub description: String,
    /// Default username
    pub default_username: String,
    /// Is the database loaded?
    pub loaded: bool,
    /// Is read-only mode enabled?
    pub is_read_only: bool,
    /// Data modified since last save?
    pub data_modified: bool,
    /// Recycle bin group UUID
    pub recycle_bin_uuid: Option<Uuid>,
    /// Entry templates group UUID
    pub entry_templates_uuid: Option<Uuid>,
    /// Default protection settings stored in Meta/MemoryProtection.
    pub memory_protection: MemoryProtectionConfig,
    /// Extensible database-level custom data.
    pub custom_data: CustomData,
    /// Set when parsing encountered XML understood only as an opaque extension.
    pub contains_unsupported_xml: bool,
    /// Opaque XML elements retained for forward-compatible round-trips.
    #[doc(hidden)]
    pub xml_extensions: DatabaseXmlExtensions,
}

impl Database {
    pub fn new(version: DatabaseVersion) -> Self {
        let file_version = match version {
            DatabaseVersion::KDB => 0x0001_0003,
            DatabaseVersion::KDBX31 => crate::kdbx::file::header::FILE_VERSION_31,
            DatabaseVersion::KDBX4 => crate::kdbx::file::header::FILE_VERSION_4,
        };
        Self {
            version,
            file_version,
            root_group_id: None,
            groups: HashMap::new(),
            entries: HashMap::new(),
            deleted_objects: Vec::new(),
            custom_icons: HashMap::new(),
            encryption_algorithm: EncryptionAlgorithm::AesRijndael,
            compression: CompressionAlgorithm::Gzip,
            kdf_parameters: None,
            public_custom_data: Vec::new(),
            header_comment: None,
            master_key_hash: None,
            name: String::new(),
            description: String::new(),
            default_username: String::new(),
            loaded: false,
            is_read_only: false,
            data_modified: false,
            recycle_bin_uuid: None,
            entry_templates_uuid: None,
            memory_protection: MemoryProtectionConfig {
                protect_password: true,
                ..MemoryProtectionConfig::default()
            },
            custom_data: CustomData::default(),
            contains_unsupported_xml: false,
            xml_extensions: DatabaseXmlExtensions::default(),
        }
    }

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

    /// Get an entry by its ID
    pub fn get_entry(&self, id: &NodeId) -> Option<&Entry> {
        self.entries.get(id)
    }

    /// Get a mutable entry by its ID
    pub fn get_entry_mut(&mut self, id: &NodeId) -> Option<&mut Entry> {
        self.entries.get_mut(id)
    }

    /// Get a group by its ID
    pub fn get_group(&self, id: &NodeId) -> Option<&Group> {
        self.groups.get(id)
    }

    /// Get a mutable group by its ID
    pub fn get_group_mut(&mut self, id: &NodeId) -> Option<&mut Group> {
        self.groups.get_mut(id)
    }

    /// Get the total number of entries
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Get the total number of groups
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// Mark the database as modified
    pub fn mark_modified(&mut self) {
        self.data_modified = true;
    }

    // ─── Entry CRUD ───

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

    /// Remove an entry from the database.
    /// If `use_recycle_bin` is true and a recycle bin exists, move to recycle bin instead.
    /// Returns the removed entry, or None if not found.
    pub fn remove_entry(&mut self, entry_id: &NodeId, use_recycle_bin: bool) -> Option<Entry> {
        // Find parent group
        let parent_id = self.find_parent_group_of_entry(entry_id)?;

        if use_recycle_bin {
            if let Some(ref recycle_uuid) = self.recycle_bin_uuid {
                let recycle_id = NodeId::from_uuid(*recycle_uuid);
                if recycle_id != *entry_id && self.groups.contains_key(&recycle_id) {
                    return self.move_entry(entry_id, &recycle_id);
                }
            }
        }

        // Permanent removal
        let entry = self.entries.remove(entry_id)?;
        // Remove from parent's child list
        if let Some(parent) = self.groups.get_mut(&parent_id) {
            parent.child_entry_ids.retain(|id| id != entry_id);
        }
        // Add to deleted objects (KDBX 4.0)
        self.deleted_objects.push(DeletedObject::new(*entry_id));
        self.mark_modified();
        Some(entry)
    }

    /// Move an entry from one group to another.
    fn move_entry(&mut self, entry_id: &NodeId, new_parent_id: &NodeId) -> Option<Entry> {
        let old_parent_id = self.find_parent_group_of_entry(entry_id)?;

        // Remove from old parent
        if let Some(parent) = self.groups.get_mut(&old_parent_id) {
            parent.child_entry_ids.retain(|id| id != entry_id);
        }
        // Add to new parent
        if let Some(parent) = self.groups.get_mut(new_parent_id) {
            parent.add_child_entry(*entry_id);
        }
        self.mark_modified();
        self.entries.get(entry_id).cloned()
    }

    // ─── Group CRUD ───

    /// Add a group to the database under the specified parent group.
    pub fn add_group(&mut self, group: Group, parent_group_id: &NodeId) -> bool {
        if parent_group_id == &group.id {
            return false; // Cannot add group as child of itself
        }
        if !self.groups.contains_key(parent_group_id) && self.root_group_id.is_none() {
            // Allow adding root group
        } else if !self.groups.contains_key(parent_group_id) {
            return false;
        }
        let group_id = group.id;
        self.groups.insert(group_id, group);
        if let Some(parent) = self.groups.get_mut(parent_group_id) {
            parent.add_child_group(group_id);
        }
        if self.root_group_id.is_none() {
            // First group becomes root
            self.root_group_id = Some(group_id);
        }
        self.mark_modified();
        true
    }

    /// Remove a group (and all its children) from the database.
    /// Cannot remove root group.
    pub fn remove_group(&mut self, group_id: &NodeId, use_recycle_bin: bool) -> Option<Group> {
        // Cannot remove root
        if let Some(rid) = self.root_group_id {
            if rid == *group_id {
                return None;
            }
        }

        if use_recycle_bin {
            if let Some(ref recycle_uuid) = self.recycle_bin_uuid {
                let recycle_id = NodeId::from_uuid(*recycle_uuid);
                if recycle_id != *group_id && self.groups.contains_key(&recycle_id) {
                    return self.move_group(group_id, &recycle_id);
                }
            }
        }

        // Find parent
        let parent_id = self.find_parent_group_of_group(group_id)?;

        // Recursively collect all descendant group IDs
        let descendants = self.collect_descendant_groups(group_id);

        // Remove all entries in this group and subgroups
        for desc_id in &descendants {
            if let Some(group) = self.groups.get(desc_id) {
                for entry_id in &group.child_entry_ids {
                    self.entries.remove(entry_id);
                    self.deleted_objects.push(DeletedObject::new(*entry_id));
                }
            }
        }

        // Remove all descendant groups (excluding self, handled last)
        for desc_id in descendants.iter().skip(1) {
            self.groups.remove(desc_id);
            self.deleted_objects.push(DeletedObject::new(*desc_id));
        }

        // Remove from parent's child list
        if let Some(parent) = self.groups.get_mut(&parent_id) {
            parent.child_group_ids.retain(|id| id != group_id);
        }

        self.deleted_objects.push(DeletedObject::new(*group_id));
        self.mark_modified();
        self.groups.remove(group_id)
    }

    /// Move a group to another parent.
    fn move_group(&mut self, group_id: &NodeId, new_parent_id: &NodeId) -> Option<Group> {
        let old_parent_id = self.find_parent_group_of_group(group_id)?;

        // Remove from old parent
        if let Some(parent) = self.groups.get_mut(&old_parent_id) {
            parent.child_group_ids.retain(|id| id != group_id);
        }
        // Add to new parent
        if let Some(parent) = self.groups.get_mut(new_parent_id) {
            parent.add_child_group(*group_id);
        }
        self.mark_modified();
        self.groups.get(group_id).cloned()
    }

    // ─── Recycle Bin ───

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

        // Add under root group
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
        let recycle_id = match self.recycle_bin_uuid {
            Some(uuid) => NodeId::from_uuid(uuid),
            None => return,
        };

        // Collect all entries and groups to delete
        let entries_to_delete: Vec<NodeId>;
        let groups_to_delete: Vec<NodeId>;

        if let Some(recycle) = self.groups.get(&recycle_id) {
            entries_to_delete = recycle.child_entry_ids.clone();
            groups_to_delete = recycle.child_group_ids.clone();
        } else {
            return;
        }

        // Remove entries
        for entry_id in &entries_to_delete {
            self.entries.remove(entry_id);
            self.deleted_objects.push(DeletedObject::new(*entry_id));
        }

        // Recursively remove subgroups
        for group_id in &groups_to_delete {
            let sub_descendants = self.collect_descendant_groups(group_id);
            for desc_id in &sub_descendants {
                if let Some(group) = self.groups.get(desc_id) {
                    for entry_id in &group.child_entry_ids {
                        self.entries.remove(entry_id);
                        self.deleted_objects.push(DeletedObject::new(*entry_id));
                    }
                }
                self.groups.remove(desc_id);
                self.deleted_objects.push(DeletedObject::new(*desc_id));
            }
        }

        // Clear recycle bin's child lists
        if let Some(recycle) = self.groups.get_mut(&recycle_id) {
            recycle.child_entry_ids.clear();
            recycle.child_group_ids.clear();
        }

        self.mark_modified();
    }

    /// Check if a group is the recycle bin.
    pub fn is_recycle_bin(&self, group_id: &NodeId) -> bool {
        match self.recycle_bin_uuid {
            Some(uuid) => *group_id == NodeId::from_uuid(uuid),
            None => false,
        }
    }

    // ─── Helper methods ───

    /// Find the parent group of an entry.
    pub fn find_parent_group_of_entry(&self, entry_id: &NodeId) -> Option<NodeId> {
        for (group_id, group) in &self.groups {
            if group.child_entry_ids.contains(entry_id) {
                return Some(*group_id);
            }
        }
        None
    }

    /// Find the parent group of a group.
    fn find_parent_group_of_group(&self, group_id: &NodeId) -> Option<NodeId> {
        for (parent_id, group) in &self.groups {
            if group.child_group_ids.contains(group_id) {
                return Some(*parent_id);
            }
        }
        None
    }

    /// Recursively collect all descendant group IDs (including self).
    fn collect_descendant_groups(&self, group_id: &NodeId) -> Vec<NodeId> {
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
        let mut visited = HashSet::new();
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
}

impl Default for Database {
    fn default() -> Self {
        Self::new(DatabaseVersion::KDBX4)
    }
}
