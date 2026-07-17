//! Entry data model
//!

pub mod auto_type;
pub mod field_references;
pub mod otp;
pub mod passkey;
pub mod versioned;

use crate::model::core::date::DateInstant;
use crate::model::core::node::{NodeId, NodeType};
use crate::model::core::security::ProtectedString;
use crate::model::meta::custom_data::CustomData;
use crate::model::meta::icon::IconImage;
use crate::model::xml::EntryXmlExtensions;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

impl crate::model::core::node::Node for Entry {
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
