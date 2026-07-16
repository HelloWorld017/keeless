//! Database root structure
//!

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::crypto::compression::CompressionAlgorithm;
use crate::crypto::encryption_algorithm::EncryptionAlgorithm;
use crate::kdbx::kdf::kdf_parameters::KdfParameters;
use crate::model::core::node::NodeId;
use crate::model::core::security::MemoryProtectionConfig;
use crate::model::entry::Entry;
use crate::model::exception::{DatabaseError, DatabaseResult};
use crate::model::group::Group;
use crate::model::meta::icon::IconImageCustom;
use crate::model::meta::{CustomData, DeletedObject};
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
#[derive(Debug)]
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
    /// Set when parsing encountered XML that cannot be losslessly rewritten.
    pub contains_unsupported_xml: bool,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::core::security::ProtectedString;

    fn make_test_db() -> Database {
        let mut db = Database::new(DatabaseVersion::KDBX4);
        let root_id = NodeId::new_uuid();
        let root = Group::new(root_id);
        db.groups.insert(root_id, root);
        db.root_group_id = Some(root_id);
        db
    }

    #[test]
    fn test_database_creation() {
        let db = Database::new(DatabaseVersion::KDBX4);
        assert!(!db.loaded);
        assert_eq!(db.entry_count(), 0);
        assert_eq!(db.group_count(), 0);
        assert_eq!(db.encryption_algorithm, EncryptionAlgorithm::AesRijndael);
    }

    #[test]
    fn test_validate_rejects_dangling_duplicate_and_cyclic_graphs() {
        let mut dangling = make_test_db();
        dangling
            .root_group_mut()
            .unwrap()
            .child_entry_ids
            .push(NodeId::new_uuid());
        assert!(matches!(
            dangling.validate(),
            Err(DatabaseError::InvalidFormat(_))
        ));

        let mut duplicate = make_test_db();
        let entry_id = NodeId::new_uuid();
        duplicate.entries.insert(entry_id, Entry::new(entry_id));
        duplicate
            .root_group_mut()
            .unwrap()
            .child_entry_ids
            .extend([entry_id, entry_id]);
        assert!(matches!(
            duplicate.validate(),
            Err(DatabaseError::InvalidFormat(_))
        ));

        let mut cyclic = make_test_db();
        let root = cyclic.root_group_id.unwrap();
        let child = NodeId::new_uuid();
        let mut child_group = Group::new(child);
        child_group.child_group_ids.push(root);
        cyclic.groups.insert(child, child_group);
        cyclic
            .groups
            .get_mut(&root)
            .unwrap()
            .child_group_ids
            .push(child);
        assert!(matches!(
            cyclic.validate(),
            Err(DatabaseError::InvalidFormat(_))
        ));
    }

    #[test]
    fn test_validate_rejects_unreachable_and_key_mismatch() {
        let mut unreachable = make_test_db();
        let group_id = NodeId::new_uuid();
        unreachable.groups.insert(group_id, Group::new(group_id));
        assert!(matches!(
            unreachable.validate(),
            Err(DatabaseError::InvalidFormat(_))
        ));

        let mut mismatch = make_test_db();
        let root = mismatch.root_group_id.unwrap();
        mismatch.groups.get_mut(&root).unwrap().id = NodeId::new_uuid();
        assert!(matches!(
            mismatch.validate(),
            Err(DatabaseError::InvalidFormat(_))
        ));
    }

    #[test]
    fn test_add_entry() {
        let mut db = make_test_db();
        let root_id = db.root_group_id.unwrap();

        let entry_id = NodeId::new_uuid();
        let mut entry = Entry::new(entry_id);
        entry.title = "Test Entry".to_string();
        entry.password = ProtectedString::new_protected("secret");

        assert!(db.add_entry(entry, &root_id));
        assert_eq!(db.entry_count(), 1);
        assert_eq!(db.get_entry(&entry_id).unwrap().title, "Test Entry");
    }

    #[test]
    fn test_add_entry_to_nonexistent_group() {
        let mut db = make_test_db();
        let fake_group = NodeId::new_uuid();
        let entry = Entry::new(NodeId::new_uuid());
        assert!(!db.add_entry(entry, &fake_group));
    }

    #[test]
    fn test_remove_entry_permanent() {
        let mut db = make_test_db();
        let root_id = db.root_group_id.unwrap();

        let entry_id = NodeId::new_uuid();
        let entry = Entry::new(entry_id);
        db.add_entry(entry, &root_id);

        let removed = db.remove_entry(&entry_id, false).unwrap();
        assert_eq!(removed.id, entry_id);
        assert_eq!(db.entry_count(), 0);
        assert!(db
            .deleted_objects
            .iter()
            .any(|deleted| deleted.id == entry_id));
    }

    #[test]
    fn test_remove_entry_to_recycle_bin() {
        let mut db = make_test_db();
        let root_id = db.root_group_id.unwrap();

        let entry_id = NodeId::new_uuid();
        let entry = Entry::new(entry_id);
        db.add_entry(entry, &root_id);

        // Create recycle bin
        let recycle_id = db.create_recycle_bin();
        assert!(db.recycle_bin_uuid.is_some());

        // Remove entry to recycle bin
        let removed = db.remove_entry(&entry_id, true).unwrap();
        assert_eq!(removed.id, entry_id);
        assert_eq!(db.entry_count(), 1); // Still in recycle bin

        let recycle_entries = db.get_entries_in_group(&recycle_id);
        assert_eq!(recycle_entries.len(), 1);
    }

    #[test]
    fn test_add_group() {
        let mut db = make_test_db();
        let root_id = db.root_group_id.unwrap();

        let group_id = NodeId::new_uuid();
        let mut group = Group::new(group_id);
        group.title = "Subgroup".to_string();

        assert!(db.add_group(group, &root_id));
        assert_eq!(db.group_count(), 2); // root + subgroup
    }

    #[test]
    fn test_remove_group() {
        let mut db = make_test_db();
        let root_id = db.root_group_id.unwrap();

        let group_id = NodeId::new_uuid();
        let group = Group::new(group_id);
        db.add_group(group, &root_id);

        let removed = db.remove_group(&group_id, false).unwrap();
        assert_eq!(removed.id, group_id);
        assert_eq!(db.group_count(), 1); // only root
    }

    #[test]
    fn test_cannot_remove_root_group() {
        let mut db = make_test_db();
        let root_id = db.root_group_id.unwrap();
        assert!(db.remove_group(&root_id, false).is_none());
    }

    #[test]
    fn test_recycle_bin_creation() {
        let mut db = make_test_db();
        assert!(db.recycle_bin_uuid.is_none());

        let recycle_id = db.create_recycle_bin();
        assert!(db.recycle_bin_uuid.is_some());
        assert_eq!(db.get_group(&recycle_id).unwrap().title, "Recycle Bin");
    }

    #[test]
    fn test_empty_recycle_bin() {
        let mut db = make_test_db();
        let root_id = db.root_group_id.unwrap();

        // Add entry and group
        let entry_id = NodeId::new_uuid();
        let entry = Entry::new(entry_id);
        db.add_entry(entry, &root_id);

        let group_id = NodeId::new_uuid();
        let group = Group::new(group_id);
        db.add_group(group, &root_id);

        // Create recycle bin
        let _recycle_id = db.create_recycle_bin();

        // Move items to recycle bin
        db.remove_entry(&entry_id, true);
        db.remove_group(&group_id, true);

        assert_eq!(db.entry_count(), 1); // entry still exists in recycle bin
        assert_eq!(db.group_count(), 3); // root + recycle + subgroup

        // Empty recycle bin
        db.empty_recycle_bin();

        assert_eq!(db.entry_count(), 0);
        assert_eq!(db.group_count(), 2); // root + empty recycle bin
    }

    #[test]
    fn test_get_all_entries_recursive() {
        let mut db = make_test_db();
        let root_id = db.root_group_id.unwrap();

        let sub_id = NodeId::new_uuid();
        let mut sub = Group::new(sub_id);
        sub.title = "Sub".to_string();
        db.add_group(sub, &root_id);

        // Add entries to root and subgroup
        let e1 = Entry::new(NodeId::new_uuid());
        let _e1_id = e1.id;
        db.add_entry(e1, &root_id);

        let e2 = Entry::new(NodeId::new_uuid());
        let _e2_id = e2.id;
        db.add_entry(e2, &sub_id);

        let all = db.get_all_entries_in_group(&root_id);
        assert_eq!(all.len(), 2);

        let root_only = db.get_entries_in_group(&root_id);
        assert_eq!(root_only.len(), 1);
    }

    #[test]
    fn test_find_parent_group() {
        let mut db = make_test_db();
        let root_id = db.root_group_id.unwrap();

        let entry_id = NodeId::new_uuid();
        let entry = Entry::new(entry_id);
        db.add_entry(entry, &root_id);

        let found = db.find_parent_group_of_entry(&entry_id);
        assert_eq!(found, Some(root_id));
    }
}
