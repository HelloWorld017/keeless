//! Database root structure
//!

mod validation;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use keeless_secure_types::SecureArray;

use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::crypto::compression::CompressionAlgorithm;
use crate::crypto::encryption_algorithm::EncryptionAlgorithm;
use crate::crypto::memory_protection::{MemoryField, MemoryProtectionContext, MemoryUnlockSession};
use crate::kdbx::file::header::{FILE_VERSION_31, FILE_VERSION_4};
use crate::kdbx::kdf::argon2_kdf::Argon2Kdf;
use crate::kdbx::kdf::kdf_engine::KdfEngine;
use crate::kdbx::kdf::kdf_parameters::KdfParameters;
use crate::model::core::date::DateInstant;
use crate::model::core::node::NodeId;
use crate::model::core::security::MemoryProtectionConfig;
use crate::model::db::composite_key::CompositeKey;
use crate::model::entry::{Entry, EntryField};
use crate::model::exception::{DatabaseError, DatabaseResult};
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

/// One field in the complete desired field list for an entry update.
#[derive(Clone, PartialEq, Eq)]
pub struct EntryFieldUpdate {
    /// Absolute index in the original entry, or `None` for a new custom field.
    pub field_index: Option<usize>,
    pub name: String,
    /// `None` preserves an existing protected value without exposing it.
    pub value: Option<String>,
    pub is_protected: bool,
}

impl Drop for EntryFieldUpdate {
    fn drop(&mut self) {
        self.value.zeroize();
    }
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
    pub(crate) memory_protection_context: Option<Arc<MemoryProtectionContext>>,
}

impl Database {
    pub fn new(version: DatabaseVersion) -> Self {
        let file_version = match version {
            DatabaseVersion::KDB => 0x0001_0003,
            DatabaseVersion::KDBX31 => FILE_VERSION_31,
            DatabaseVersion::KDBX4 => FILE_VERSION_4,
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
            memory_protection_context: None,
        }
    }

    /// Encrypt all KDBX-protected entry strings before exposing a loaded database.
    pub(crate) fn seal_protected_strings(
        &mut self,
        composite_key: &CompositeKey,
    ) -> DatabaseResult<()> {
        let context = match &self.memory_protection_context {
            Some(context) => {
                let context = context.clone();
                let mut unlock = MemoryUnlockSession::new(composite_key);
                unlock.with_root(&context, |root| {
                    for entry in self.entries.values_mut() {
                        entry.seal_protected_strings(context.clone(), root)?;
                    }
                    Ok(())
                })?;
                context
            }
            None => {
                let (context, root) = self.create_memory_context(composite_key)?;
                root.unlock(|root| {
                    for entry in self.entries.values_mut() {
                        entry.seal_protected_strings(context.clone(), root)?;
                    }
                    Ok::<_, DatabaseError>(())
                })??;
                context
            }
        };
        self.memory_protection_context = Some(context);
        Ok(())
    }

    /// Seal protected strings added through low-level model APIs.
    pub fn protect_entry_strings(&mut self, composite_key: &CompositeKey) -> DatabaseResult<()> {
        self.seal_protected_strings(composite_key)
    }

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

    /// Atomically replace all standard fields and the complete ordered custom-field list.
    /// Returns `false` when the requested representation is semantically unchanged.
    pub fn update_entry_fields(
        &mut self,
        composite_key: &CompositeKey,
        entry_id: &NodeId,
        fields: &[EntryFieldUpdate],
    ) -> DatabaseResult<bool> {
        const STANDARD_NAMES: [&str; 5] = ["Title", "UserName", "Password", "URL", "Notes"];

        let original = self
            .entries
            .get(entry_id)
            .ok_or_else(|| DatabaseError::InvalidFormat("entry does not exist".into()))?
            .clone();
        let source_count = 5 + original.custom_fields.len();
        let mut source_indices = HashSet::new();
        let mut standard_seen = [false; 5];
        for field in fields {
            match field.field_index {
                Some(index) if index < source_count && source_indices.insert(index) => {
                    if index < 5 {
                        if field.name != STANDARD_NAMES[index] {
                            return Err(DatabaseError::InvalidFormat(
                                "standard entry field was renamed".into(),
                            ));
                        }
                        standard_seen[index] = true;
                    } else if STANDARD_NAMES.contains(&field.name.as_str()) {
                        return Err(DatabaseError::InvalidFormat(
                            "standard entry field was duplicated".into(),
                        ));
                    }
                }
                Some(_) => {
                    return Err(DatabaseError::InvalidFormat(
                        "entry field source index is invalid or duplicated".into(),
                    ));
                }
                None if field.value.is_none() => {
                    return Err(DatabaseError::InvalidFormat(
                        "new entry fields require a value".into(),
                    ));
                }
                None if STANDARD_NAMES.contains(&field.name.as_str()) => {
                    return Err(DatabaseError::InvalidFormat(
                        "standard entry field was duplicated".into(),
                    ));
                }
                None => {}
            }
        }
        if !standard_seen.into_iter().all(|seen| seen) {
            return Err(DatabaseError::InvalidFormat(
                "all standard entry fields are required".into(),
            ));
        }

        let context = self
            .memory_protection_context
            .clone()
            .map(Ok)
            .unwrap_or_else(|| {
                self.create_memory_context(composite_key)
                    .map(|(context, _)| context)
            })?;
        let mut updated = original.clone();
        updated.custom_fields.clear();
        updated.xml_extensions.custom_strings.clear();
        let retained_custom_count = fields
            .iter()
            .filter(|field| field.field_index.is_some_and(|index| index >= 5))
            .count();
        let mut changed = retained_custom_count != original.custom_fields.len();
        let mut custom_position = 5;
        let mut unlock = MemoryUnlockSession::new(composite_key);
        let mut plaintexts = Vec::with_capacity(fields.len());
        for requested in fields {
            let Some(index) = requested.field_index else {
                plaintexts.push(None);
                changed = true;
                continue;
            };
            if index >= 5 {
                changed |= index != custom_position;
                custom_position += 1;
            }
            let (source_value, source_field) = entry_value_at(&original, index);
            if requested.value.is_none() && !source_value.is_protected() {
                return Err(DatabaseError::InvalidFormat(
                    "only protected entry fields can preserve a hidden value".into(),
                ));
            }
            let plaintext =
                source_value.with_plaintext(&mut unlock, original.id, &source_field, |value| {
                    Ok(Zeroizing::new(value.to_string()))
                })?;
            let target_field = entry_memory_field(index, &requested.name);
            changed |= requested
                .value
                .as_deref()
                .is_some_and(|value| value != *plaintext)
                || source_value.is_protected() != requested.is_protected
                || source_field != target_field;
            plaintexts.push(Some(plaintext));
        }

        unlock.with_root(&context, |root| {
            for (requested, plaintext) in fields.iter().zip(&plaintexts) {
                let (source_value, source_field) = match requested.field_index {
                    Some(index) => entry_value_at(&original, index),
                    None => {
                        let value = requested
                            .value
                            .as_deref()
                            .expect("new field value validated");
                        let mut target =
                            crate::model::core::security::ProtectedString::new_plain(value);
                        replace_entry_value(
                            &mut target,
                            context.clone(),
                            root,
                            original.id,
                            &MemoryField::Custom(requested.name.clone()),
                            value,
                            requested.is_protected,
                        )?;
                        updated.custom_fields.push(EntryField {
                            name: requested.name.clone(),
                            value: target,
                        });
                        updated.xml_extensions.custom_strings.push(Vec::new());
                        changed = true;
                        continue;
                    }
                };
                let index = requested.field_index.expect("existing source");
                let target_field = entry_memory_field(index, &requested.name);

                let mut target = if requested.value.is_none()
                    && source_field == target_field
                    && requested.is_protected
                {
                    source_value.clone()
                } else {
                    let value = requested
                        .value
                        .as_deref()
                        .unwrap_or_else(|| plaintext.as_ref().expect("existing plaintext"));
                    let mut target =
                        crate::model::core::security::ProtectedString::new_plain(value);
                    replace_entry_value(
                        &mut target,
                        context.clone(),
                        root,
                        original.id,
                        &target_field,
                        value,
                        requested.is_protected,
                    )?;
                    target
                };

                match requested.field_index.expect("existing source") {
                    0 => updated.title = target,
                    1 => updated.username = target,
                    2 => updated.password = target,
                    3 => updated.url = target,
                    4 => updated.notes = target,
                    _ => {
                        updated.custom_fields.push(EntryField {
                            name: requested.name.clone(),
                            value: std::mem::take(&mut target),
                        });
                        updated.xml_extensions.custom_strings.push(
                            original
                                .xml_extensions
                                .custom_strings
                                .get(index - 5)
                                .cloned()
                                .unwrap_or_default(),
                        );
                    }
                }
            }
            Ok(())
        })?;

        if !changed {
            return Ok(false);
        }
        let mut snapshot = original;
        snapshot.history.clear();
        snapshot.xml_extensions.history.clear();
        updated.history.push(snapshot);
        if updated.history.len() > 10 {
            updated.history.remove(0);
        }
        updated.last_modification_time = DateInstant::now();
        if self.memory_protection_context.is_none() {
            self.memory_protection_context = Some(context);
        }
        self.entries.insert(*entry_id, updated);
        self.mark_modified();
        Ok(true)
    }

    pub(crate) fn memory_unlock<'a>(
        &self,
        composite_key: &'a CompositeKey,
    ) -> MemoryUnlockSession<'a> {
        MemoryUnlockSession::new(composite_key)
    }

    fn create_memory_context(
        &self,
        composite_key: &CompositeKey,
    ) -> DatabaseResult<(Arc<MemoryProtectionContext>, SecureArray<32>)> {
        let parameters = self.kdf_parameters.clone().unwrap_or_else(|| {
            let kdf = Argon2Kdf::argon2id();
            kdf.default_parameters()
        });
        MemoryProtectionContext::create(composite_key, parameters)
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

    /// Move an entry from one group to another.
    fn move_entry(&mut self, entry_id: &NodeId, new_parent_id: &NodeId) -> Option<Entry> {
        self.reposition_entry(entry_id, new_parent_id)
            .then(|| self.entries.get(entry_id).cloned())
            .flatten()
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

fn replace_entry_value(
    target: &mut crate::model::core::security::ProtectedString,
    context: Arc<MemoryProtectionContext>,
    root: &[u8; 32],
    entry_id: NodeId,
    field: &MemoryField,
    value: &str,
    protected: bool,
) -> DatabaseResult<()> {
    if protected {
        target.replace_sealed(context, root, entry_id, field, value)
    } else {
        target.replace_plain(value);
        Ok(())
    }
}

fn entry_value_at(
    entry: &Entry,
    index: usize,
) -> (&crate::model::core::security::ProtectedString, MemoryField) {
    match index {
        0 => (&entry.title, MemoryField::Title),
        1 => (&entry.username, MemoryField::UserName),
        2 => (&entry.password, MemoryField::Password),
        3 => (&entry.url, MemoryField::Url),
        4 => (&entry.notes, MemoryField::Notes),
        _ => {
            let field = &entry.custom_fields[index - 5];
            (&field.value, MemoryField::Custom(field.name.clone()))
        }
    }
}

fn entry_memory_field(index: usize, name: &str) -> MemoryField {
    match index {
        0 => MemoryField::Title,
        1 => MemoryField::UserName,
        2 => MemoryField::Password,
        3 => MemoryField::Url,
        4 => MemoryField::Notes,
        _ => MemoryField::Custom(name.to_string()),
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new(DatabaseVersion::KDBX4)
    }
}
