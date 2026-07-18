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

use crate::crypto::memory_protection::{
    EncryptedValue, MemoryField, MemoryProtectionContext, MemoryUnlockSession,
};
use crate::model::exception::{DatabaseError, DatabaseResult};

pub use auto_type::{AutoType, AutoTypeAssociation};
pub use field_references::{FieldReference, RefTarget};

/// A KeePass database entry (password record).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    /// Unique identifier
    pub id: NodeId,
    /// Entry title
    pub title: String,
    /// XML protection state for the title field.
    pub title_is_protected: bool,
    /// User name
    pub username: ProtectedString,
    /// Password
    pub password: ProtectedString,
    /// URL
    pub url: String,
    /// XML protection state for the URL field.
    pub url_is_protected: bool,
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
    #[serde(skip)]
    pub(crate) protected_title: Option<EncryptedValue>,
    #[serde(skip)]
    pub(crate) protected_url: Option<EncryptedValue>,
}

/// A custom field in an entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntryField {
    pub name: String,
    pub value: ProtectedString,
    pub is_protected: bool,
}

/// An entry attachment and its KDBX4 inner-header protection state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntryBinary {
    pub name: String,
    pub data: Vec<u8>,
    pub is_protected: bool,
}

impl Drop for Entry {
    fn drop(&mut self) {
        if self.title_is_protected {
            self.title.zeroize();
        }
        if self.url_is_protected {
            self.url.zeroize();
        }
    }
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
            title: String::new(),
            title_is_protected: false,
            username: ProtectedString::new(),
            password: ProtectedString::new(),
            url: String::new(),
            url_is_protected: false,
            notes: ProtectedString::new(),
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
            protected_title: None,
            protected_url: None,
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
            EntryFieldSelector::Title => self
                .protected_title
                .is_none()
                .then(|| use_value(&self.title)),
            EntryFieldSelector::UserName => {
                (!self.username.is_memory_protected()).then(|| use_value(self.username.as_str()))
            }
            EntryFieldSelector::Password => {
                (!self.password.is_memory_protected()).then(|| use_value(self.password.as_str()))
            }
            EntryFieldSelector::Url => self.protected_url.is_none().then(|| use_value(&self.url)),
            EntryFieldSelector::Notes => {
                (!self.notes.is_memory_protected()).then(|| use_value(self.notes.as_str()))
            }
            EntryFieldSelector::Custom(name) => {
                let field = self
                    .custom_fields
                    .iter()
                    .find(|candidate| candidate.name == *name)?;
                (!field.value.is_memory_protected()).then(|| use_value(field.value.as_str()))
            }
        }
    }

    /// Get the standard field value by name.
    pub fn get_field(&self, name: &str) -> Option<ProtectedString> {
        match name.to_lowercase().as_str() {
            "title" => Some(ProtectedString::new_plain(&self.title)),
            "username" | "user name" => Some(self.username.clone()),
            "password" => Some(self.password.clone()),
            "url" => Some(ProtectedString::new_plain(&self.url)),
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
        if self.title_is_protected && self.protected_title.is_none() {
            self.protected_title = Some(EncryptedValue::encrypt(
                context.clone(),
                root,
                self.id,
                &MemoryField::Title,
                self.title.as_bytes(),
            )?);
            self.title.zeroize();
            self.title.clear();
        }
        self.username
            .seal(context.clone(), root, self.id, &MemoryField::UserName)?;
        self.password
            .seal(context.clone(), root, self.id, &MemoryField::Password)?;
        if self.url_is_protected && self.protected_url.is_none() {
            self.protected_url = Some(EncryptedValue::encrypt(
                context.clone(),
                root,
                self.id,
                &MemoryField::Url,
                self.url.as_bytes(),
            )?);
            self.url.zeroize();
            self.url.clear();
        }
        self.notes
            .seal(context.clone(), root, self.id, &MemoryField::Notes)?;
        for field in &mut self.custom_fields {
            if field.is_protected {
                field.value.seal_as_protected(
                    context.clone(),
                    root,
                    self.id,
                    &MemoryField::Custom(field.name.clone()),
                )?;
            }
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
            MemoryField::Title => {
                if let Some(value) = &self.protected_title {
                    unlock.with_root(&value.context, |root| {
                        let plaintext = value.decrypt(root, self.id, field)?;
                        let text = std::str::from_utf8(plaintext.as_slice()).map_err(|err| {
                            DatabaseError::DecryptionError(format!(
                                "memory-protected title is not UTF-8: {err}"
                            ))
                        })?;
                        use_value(text)
                    })
                } else {
                    use_value(&self.title)
                }
            }
            MemoryField::UserName => self
                .username
                .with_plaintext(unlock, self.id, field, use_value),
            MemoryField::Password => self
                .password
                .with_plaintext(unlock, self.id, field, use_value),
            MemoryField::Url => {
                if let Some(value) = &self.protected_url {
                    unlock.with_root(&value.context, |root| {
                        let plaintext = value.decrypt(root, self.id, field)?;
                        let text = std::str::from_utf8(plaintext.as_slice()).map_err(|err| {
                            DatabaseError::DecryptionError(format!(
                                "memory-protected URL is not UTF-8: {err}"
                            ))
                        })?;
                        use_value(text)
                    })
                } else {
                    use_value(&self.url)
                }
            }
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
            MemoryField::Title => {
                self.title_is_protected = protected;
                if protected {
                    self.protected_title = Some(EncryptedValue::encrypt(
                        context,
                        root,
                        self.id,
                        field,
                        value.as_bytes(),
                    )?);
                    self.title.zeroize();
                    self.title.clear();
                } else {
                    self.protected_title = None;
                    self.title = value.to_string();
                }
            }
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
            MemoryField::Url => {
                self.url_is_protected = protected;
                if protected {
                    self.protected_url = Some(EncryptedValue::encrypt(
                        context,
                        root,
                        self.id,
                        field,
                        value.as_bytes(),
                    )?);
                    self.url.zeroize();
                    self.url.clear();
                } else {
                    self.protected_url = None;
                    self.url = value.to_string();
                }
            }
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
                target.is_protected = protected;
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
        let title =
            self.with_memory_field(unlock, &MemoryField::Title, |value| Ok(value.to_string()))?;
        clone.title = title;
        clone.protected_title = None;

        for (field, target) in [
            (MemoryField::UserName, &mut clone.username),
            (MemoryField::Password, &mut clone.password),
            (MemoryField::Notes, &mut clone.notes),
        ] {
            let value = self.with_memory_field(unlock, &field, |value| Ok(value.to_string()))?;
            if target.is_protected() {
                target.replace_unsealed(&value);
            } else {
                target.replace_plain(&value);
            }
        }

        let url =
            self.with_memory_field(unlock, &MemoryField::Url, |value| Ok(value.to_string()))?;
        clone.url = url;
        clone.protected_url = None;

        for (source, target) in self.custom_fields.iter().zip(&mut clone.custom_fields) {
            let field = MemoryField::Custom(source.name.clone());
            let value = self.with_memory_field(unlock, &field, |value| Ok(value.to_string()))?;
            if target.is_protected {
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
        if let Some(value) = self.protected_title.take() {
            let context = value.context.clone();
            self.protected_title = Some(unlock.with_root(&context, |root| {
                let plaintext = value.decrypt(root, old_entry_id, &MemoryField::Title)?;
                EncryptedValue::encrypt(
                    context.clone(),
                    root,
                    new_entry_id,
                    &MemoryField::Title,
                    plaintext.as_slice(),
                )
            })?);
        }
        self.username
            .rebind(unlock, old_entry_id, new_entry_id, &MemoryField::UserName)?;
        self.password
            .rebind(unlock, old_entry_id, new_entry_id, &MemoryField::Password)?;
        if let Some(value) = self.protected_url.take() {
            let context = value.context.clone();
            self.protected_url = Some(unlock.with_root(&context, |root| {
                let plaintext = value.decrypt(root, old_entry_id, &MemoryField::Url)?;
                EncryptedValue::encrypt(
                    context.clone(),
                    root,
                    new_entry_id,
                    &MemoryField::Url,
                    plaintext.as_slice(),
                )
            })?);
        }
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
        &self.title
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
        entry.title = "Test".to_string();
        entry.username = ProtectedString::new_plain("user123");

        assert_eq!(entry.get_field("Title").unwrap().as_str(), "Test");
        assert_eq!(entry.get_field("UserName").unwrap().as_str(), "user123");
    }

    #[test]
    fn test_entry_push_history() {
        let mut entry = Entry::new(NodeId::new_uuid());
        entry.title = "V1".to_string();

        entry.push_history();
        entry.title = "V2".to_string();

        assert_eq!(entry.history_count(), 1);
        assert_eq!(entry.history[0].title, "V1");
        assert_eq!(entry.title, "V2");
    }

    #[test]
    fn test_entry_restore_from_history() {
        let mut entry = Entry::new(NodeId::new_uuid());
        let id = entry.id;
        entry.title = "Original".to_string();

        entry.push_history();
        entry.title = "Modified".to_string();

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
            entry.title = format!("V{}", i);
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
