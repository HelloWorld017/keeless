//! Entry data model
//!

pub mod auto_type;
pub mod field_references;
pub mod otp;
pub mod passkey;
pub mod versioned;

use crate::model::core::date::DateInstant;
use crate::model::core::node::{Node, NodeId, NodeType};
use crate::model::core::security::ProtectedString;
use crate::model::db::EntryFieldSelector;
use crate::model::meta::custom_data::CustomData;
use crate::model::meta::icon::IconImage;
use crate::model::xml::EntryXmlExtensions;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroize;

use crate::crypto::memory_protection::{MemoryField, MemoryProtectionContext, MemoryUnlockSession};
use crate::model::exception::{DatabaseError, DatabaseResult};

pub use auto_type::{AutoType, AutoTypeAssociation};
pub use field_references::{FieldReference, RefTarget};

/// A KeePass database entry (password record).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    /// Unique identifier
    pub id: NodeId,
    /// Entry title
    pub title: ProtectedString,
    /// User name
    pub username: ProtectedString,
    /// Password
    pub password: ProtectedString,
    /// URL
    pub url: ProtectedString,
    /// Notes
    pub notes: ProtectedString,
    /// Icon
    pub icon: IconImage,
    /// Custom icon UUID
    pub custom_icon_uuid: Option<Uuid>,
    /// Background color
    pub background_color: String,
    /// Foreground color
    pub foreground_color: String,
    /// Override URL
    pub override_url: String,
    /// Tags
    pub tags: Vec<String>,
    /// Custom fields
    pub custom_fields: Vec<EntryField>,
    /// Binary attachments
    pub binaries: Vec<EntryBinary>,
    /// Creation time
    pub creation_time: DateInstant,
    /// Last modification time
    pub last_modification_time: DateInstant,
    /// Last access time
    pub last_access_time: DateInstant,
    /// Last location change time.
    pub location_changed: DateInstant,
    /// Expiry time
    pub expiry_time: DateInstant,
    /// Whether the entry expires
    pub expires: bool,
    /// Usage count
    pub usage_count: i64,
    /// History entries
    pub history: Vec<Entry>,
    /// Auto-type configuration.
    pub auto_type: AutoType,
    /// Extensible KDBX custom data.
    pub custom_data: CustomData,
    /// Is this a template entry?
    pub is_template: bool,
    /// Opaque XML elements retained for forward-compatible round-trips.
    #[doc(hidden)]
    #[serde(skip)]
    pub xml_extensions: EntryXmlExtensions,
}

/// A custom field in an entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntryField {
    pub name: String,
    pub value: ProtectedString,
}

/// An entry attachment and its KDBX4 inner-header protection state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntryBinary {
    pub name: String,
    pub data: Vec<u8>,
    pub is_protected: bool,
}

impl Drop for EntryBinary {
    fn drop(&mut self) {
        if self.is_protected {
            self.data.zeroize();
        }
    }
}

impl Entry {
    pub fn new(id: NodeId) -> Self {
        let now = DateInstant::now();
        Self {
            id,
            title: ProtectedString::new_plain(""),
            username: ProtectedString::new_plain(""),
            password: ProtectedString::new(),
            url: ProtectedString::new_plain(""),
            notes: ProtectedString::new_plain(""),
            icon: IconImage::default(),
            custom_icon_uuid: None,
            background_color: String::new(),
            foreground_color: String::new(),
            override_url: String::new(),
            tags: Vec::new(),
            custom_fields: Vec::new(),
            binaries: Vec::new(),
            creation_time: now,
            last_modification_time: now,
            last_access_time: now,
            location_changed: now,
            expiry_time: DateInstant::never(),
            expires: false,
            usage_count: 0,
            history: Vec::new(),
            auto_type: AutoType::default(),
            custom_data: CustomData::default(),
            is_template: false,
            xml_extensions: EntryXmlExtensions::default(),
        }
    }

    /// Use an entry field only when its plaintext is not sealed in memory.
    ///
    /// This does not require credentials. It returns `None` when the field is
    /// sealed or when a selected custom field does not exist.
    pub fn with_unsealed_field<T>(
        &self,
        selector: &EntryFieldSelector,
        use_value: impl FnOnce(&str) -> T,
    ) -> Option<T> {
        match selector {
            EntryFieldSelector::Title => self.title.as_unsealed_str().map(use_value),
            EntryFieldSelector::UserName => self.username.as_unsealed_str().map(use_value),
            EntryFieldSelector::Password => self.password.as_unsealed_str().map(use_value),
            EntryFieldSelector::Url => self.url.as_unsealed_str().map(use_value),
            EntryFieldSelector::Notes => self.notes.as_unsealed_str().map(use_value),
            EntryFieldSelector::Custom(name) => {
                let field = self
                    .custom_fields
                    .iter()
                    .find(|candidate| candidate.name == *name)?;
                field.value.as_unsealed_str().map(use_value)
            }
        }
    }

    /// Get the standard field value by name.
    pub fn get_field(&self, name: &str) -> Option<ProtectedString> {
        match name.to_lowercase().as_str() {
            "title" => Some(self.title.clone()),
            "username" | "user name" => Some(self.username.clone()),
            "password" => Some(self.password.clone()),
            "url" => Some(self.url.clone()),
            "notes" => Some(self.notes.clone()),
            _ => self
                .custom_fields
                .iter()
                .find(|f| f.name == name)
                .map(|f| f.value.clone()),
        }
    }

    pub(crate) fn seal_protected_strings(
        &mut self,
        context: std::sync::Arc<MemoryProtectionContext>,
        root: &[u8; 32],
    ) -> DatabaseResult<()> {
        self.title
            .seal(context.clone(), root, self.id, &MemoryField::Title)?;
        self.username
            .seal(context.clone(), root, self.id, &MemoryField::UserName)?;
        self.password
            .seal(context.clone(), root, self.id, &MemoryField::Password)?;
        self.url
            .seal(context.clone(), root, self.id, &MemoryField::Url)?;
        self.notes
            .seal(context.clone(), root, self.id, &MemoryField::Notes)?;
        for field in &mut self.custom_fields {
            field.value.seal(
                context.clone(),
                root,
                self.id,
                &MemoryField::Custom(field.name.clone()),
            )?;
        }
        for history in &mut self.history {
            history.seal_protected_strings(context.clone(), root)?;
        }
        Ok(())
    }

    pub(crate) fn with_memory_field<T>(
        &self,
        unlock: &mut MemoryUnlockSession<'_>,
        field: &MemoryField,
        use_value: impl FnOnce(&str) -> DatabaseResult<T>,
    ) -> DatabaseResult<T> {
        match field {
            MemoryField::Title => self.title.with_plaintext(unlock, self.id, field, use_value),
            MemoryField::UserName => self
                .username
                .with_plaintext(unlock, self.id, field, use_value),
            MemoryField::Password => self
                .password
                .with_plaintext(unlock, self.id, field, use_value),
            MemoryField::Url => self.url.with_plaintext(unlock, self.id, field, use_value),
            MemoryField::Notes => self.notes.with_plaintext(unlock, self.id, field, use_value),
            MemoryField::Custom(name) => {
                let value = self
                    .custom_fields
                    .iter()
                    .find(|candidate| candidate.name == *name)
                    .ok_or_else(|| {
                        DatabaseError::InvalidFormat(format!("unknown field: {name}"))
                    })?;
                value
                    .value
                    .with_plaintext(unlock, self.id, field, use_value)
            }
        }
    }

    pub(crate) fn replace_memory_field(
        &mut self,
        context: std::sync::Arc<MemoryProtectionContext>,
        root: &[u8; 32],
        field: &MemoryField,
        value: &str,
        protected: bool,
    ) -> DatabaseResult<()> {
        match field {
            MemoryField::Title => replace_protected_string(
                &mut self.title,
                context,
                root,
                self.id,
                field,
                value,
                protected,
            )?,
            MemoryField::UserName => replace_protected_string(
                &mut self.username,
                context,
                root,
                self.id,
                field,
                value,
                protected,
            )?,
            MemoryField::Password => replace_protected_string(
                &mut self.password,
                context,
                root,
                self.id,
                field,
                value,
                protected,
            )?,
            MemoryField::Url => replace_protected_string(
                &mut self.url,
                context,
                root,
                self.id,
                field,
                value,
                protected,
            )?,
            MemoryField::Notes => replace_protected_string(
                &mut self.notes,
                context,
                root,
                self.id,
                field,
                value,
                protected,
            )?,
            MemoryField::Custom(name) => {
                let target = self
                    .custom_fields
                    .iter_mut()
                    .find(|candidate| candidate.name == *name)
                    .ok_or_else(|| {
                        DatabaseError::InvalidFormat(format!("unknown field: {name}"))
                    })?;
                replace_protected_string(
                    &mut target.value,
                    context,
                    root,
                    self.id,
                    field,
                    value,
                    protected,
                )?;
            }
        }
        Ok(())
    }

    pub(crate) fn semantic_clone(
        &self,
        unlock: &mut MemoryUnlockSession<'_>,
    ) -> DatabaseResult<Self> {
        let mut clone = self.clone();
        for (field, target) in [
            (MemoryField::Title, &mut clone.title),
            (MemoryField::UserName, &mut clone.username),
            (MemoryField::Password, &mut clone.password),
            (MemoryField::Url, &mut clone.url),
            (MemoryField::Notes, &mut clone.notes),
        ] {
            let value = self.with_memory_field(unlock, &field, |value| Ok(value.to_string()))?;
            if target.is_protected() {
                target.replace_unsealed(&value);
            } else {
                target.replace_plain(&value);
            }
        }

        for (source, target) in self.custom_fields.iter().zip(&mut clone.custom_fields) {
            let field = MemoryField::Custom(source.name.clone());
            let value = self.with_memory_field(unlock, &field, |value| Ok(value.to_string()))?;
            if target.value.is_protected() {
                target.value.replace_unsealed(&value);
            } else {
                target.value.replace_plain(&value);
            }
        }

        clone.history = self
            .history
            .iter()
            .map(|history| history.semantic_clone(unlock))
            .collect::<DatabaseResult<Vec<_>>>()?;
        Ok(clone)
    }

    pub(crate) fn rebind_memory_protection(
        &mut self,
        unlock: &mut MemoryUnlockSession<'_>,
        new_entry_id: NodeId,
    ) -> DatabaseResult<()> {
        let old_entry_id = self.id;
        self.title
            .rebind(unlock, old_entry_id, new_entry_id, &MemoryField::Title)?;
        self.username
            .rebind(unlock, old_entry_id, new_entry_id, &MemoryField::UserName)?;
        self.password
            .rebind(unlock, old_entry_id, new_entry_id, &MemoryField::Password)?;
        self.url
            .rebind(unlock, old_entry_id, new_entry_id, &MemoryField::Url)?;
        self.notes
            .rebind(unlock, old_entry_id, new_entry_id, &MemoryField::Notes)?;
        for field in &mut self.custom_fields {
            field.value.rebind(
                unlock,
                old_entry_id,
                new_entry_id,
                &MemoryField::Custom(field.name.clone()),
            )?;
        }
        for history in &mut self.history {
            history.rebind_memory_protection(unlock, new_entry_id)?;
        }
        self.id = new_entry_id;
        for history in &mut self.history {
            history.id = new_entry_id;
        }
        Ok(())
    }

    pub(crate) fn prepare_duplicate(
        &mut self,
        new_entry_id: NodeId,
        unlock: Option<&mut MemoryUnlockSession<'_>>,
    ) -> DatabaseResult<()> {
        self.history.clear();
        self.xml_extensions.history.clear();

        if let Some(unlock) = unlock {
            self.rebind_memory_protection(unlock, new_entry_id)?;
        } else {
            if self.title.is_protected() {
                self.title = ProtectedString::new_protected("");
            }
            if self.username.is_protected() {
                self.username = ProtectedString::new_protected("");
            }
            if self.password.is_protected() {
                self.password = ProtectedString::new_protected("");
            }
            if self.url.is_protected() {
                self.url = ProtectedString::new_protected("");
            }
            if self.notes.is_protected() {
                self.notes = ProtectedString::new_protected("");
            }
            for field in &mut self.custom_fields {
                if field.value.is_protected() {
                    field.value = ProtectedString::new_protected("");
                }
            }
            self.binaries.retain(|binary| !binary.is_protected);
            self.id = new_entry_id;
        }

        let now = DateInstant::now();
        self.creation_time = now;
        self.last_modification_time = now;
        self.last_access_time = now;
        self.location_changed = now;
        self.usage_count = 0;
        self.is_template = false;
        Ok(())
    }

    /// Create a snapshot of this entry for history.
    /// The snapshot is a clone with history cleared and a fresh modification time.
    pub fn create_snapshot(&self) -> Self {
        let mut snapshot = self.clone();
        snapshot.history = Vec::new(); // Don't nest history
        snapshot.last_modification_time = DateInstant::now();
        snapshot
    }

    /// Push the current state into history and apply modifications.
    /// Returns the previous state as a history entry.
    pub fn push_history(&mut self) {
        let snapshot = self.create_snapshot();
        self.history.push(snapshot);

        // Limit history size to 10 entries (KeePass default)
        const MAX_HISTORY_SIZE: usize = 10;
        if self.history.len() > MAX_HISTORY_SIZE {
            self.history.remove(0);
        }
    }

    /// Restore from a history entry by index.
    /// Returns true if successful.
    pub fn restore_from_history(&mut self, index: usize) -> bool {
        if index >= self.history.len() {
            return false;
        }

        // Save current state to history before restoring
        let current = self.create_snapshot();
        let restored = self.history.remove(index);
        let restored_id = self.id; // Keep current ID

        *self = restored;
        self.id = restored_id;
        self.history.push(current);

        true
    }

    /// Clear all history entries.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Get the number of history entries.
    pub fn history_count(&self) -> usize {
        self.history.len()
    }
}

fn replace_protected_string(
    target: &mut ProtectedString,
    context: std::sync::Arc<MemoryProtectionContext>,
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

impl Node for Entry {
    fn node_id(&self) -> &NodeId {
        &self.id
    }

    fn node_type(&self) -> NodeType {
        NodeType::Entry
    }

    fn title(&self) -> &str {
        self.title.as_unsealed_str().unwrap_or("")
    }

    fn last_modified(&self) -> i64 {
        self.last_modification_time.as_millis().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_creation() {
        let entry = Entry::new(NodeId::new_uuid());
        assert!(entry.title.is_empty());
        assert!(entry.tags.is_empty());
        assert!(!entry.expires);
    }

    #[test]
    fn test_entry_get_field() {
        let mut entry = Entry::new(NodeId::new_uuid());
        entry.title = "Test".into();
        entry.username = ProtectedString::new_plain("user123");

        assert_eq!(entry.get_field("Title").unwrap().as_str(), "Test");
        assert_eq!(entry.get_field("UserName").unwrap().as_str(), "user123");
    }

    #[test]
    fn test_entry_push_history() {
        let mut entry = Entry::new(NodeId::new_uuid());
        entry.title = "V1".into();

        entry.push_history();
        entry.title = "V2".into();

        assert_eq!(entry.history_count(), 1);
        assert_eq!(entry.history[0].title, "V1");
        assert_eq!(entry.title, "V2");
    }

    #[test]
    fn test_entry_restore_from_history() {
        let mut entry = Entry::new(NodeId::new_uuid());
        let id = entry.id;
        entry.title = "Original".into();

        entry.push_history();
        entry.title = "Modified".into();

        assert_eq!(entry.title, "Modified");
        assert!(entry.restore_from_history(0));
        assert_eq!(entry.title, "Original");
        assert_eq!(entry.id, id); // ID preserved
    }

    #[test]
    fn test_entry_history_limit() {
        let mut entry = Entry::new(NodeId::new_uuid());
        for i in 0..15 {
            entry.push_history();
            entry.title = format!("V{}", i).into();
        }
        assert_eq!(entry.history_count(), 10); // Limited to 10
    }

    #[test]
    fn test_entry_clear_history() {
        let mut entry = Entry::new(NodeId::new_uuid());
        entry.push_history();
        entry.push_history();
        assert_eq!(entry.history_count(), 2);

        entry.clear_history();
        assert_eq!(entry.history_count(), 0);
    }
}
