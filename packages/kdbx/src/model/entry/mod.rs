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
use crate::model::meta::etm::{parse_template_uuid, ETM_TEMPLATE};
use crate::model::meta::icon::IconImage;
use crate::model::xml::{EntryXmlExtensions, PreservedXmlElement};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;
use zeroize::Zeroize;

use crate::crypto::memory_protection::{MemoryField, MemoryProtectionContext, MemoryUnlockSession};
use crate::model::exception::{DatabaseError, DatabaseResult};

pub use auto_type::{AutoType, AutoTypeAssociation};
pub use field_references::{FieldReference, RefTarget};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardField {
    Title,
    UserName,
    Password,
    Url,
    Notes,
}

impl StandardField {
    pub(crate) const ALL: [Self; 5] = [
        Self::Title,
        Self::UserName,
        Self::Password,
        Self::Url,
        Self::Notes,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::UserName => "UserName",
            Self::Password => "Password",
            Self::Url => "URL",
            Self::Notes => "Notes",
        }
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "Title" => Some(Self::Title),
            "UserName" => Some(Self::UserName),
            "Password" => Some(Self::Password),
            "URL" => Some(Self::Url),
            "Notes" => Some(Self::Notes),
            _ => None,
        }
    }

    fn default_value(self) -> ProtectedString {
        if self == Self::Password {
            ProtectedString::new()
        } else {
            ProtectedString::new_plain("")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryFieldId {
    Standard(StandardField),
    Custom(Uuid),
}

impl fmt::Display for EntryFieldId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Standard(field) => write!(formatter, "standard:{}", field.name()),
            Self::Custom(id) => id.fmt(formatter),
        }
    }
}

impl FromStr for EntryFieldId {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some(name) = value.strip_prefix("standard:") {
            return StandardField::from_name(name).map(Self::Standard).ok_or(());
        }
        Uuid::parse_str(value).map(Self::Custom).map_err(|_| ())
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EntryFields(pub(crate) IndexMap<EntryFieldId, EntryField>);

impl PartialEq for EntryFields {
    fn eq(&self, other: &Self) -> bool {
        self.0.values().eq(other.0.values())
    }
}

impl Eq for EntryFields {}

impl Serialize for EntryFields {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.values().collect::<Vec<_>>().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EntryFields {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let fields = Vec::<EntryField>::deserialize(deserializer)?;
        let mut result = Self::default();
        for field in fields {
            let id = StandardField::from_name(&field.name)
                .map(EntryFieldId::Standard)
                .unwrap_or_else(|| EntryFieldId::Custom(Uuid::new_v4()));
            if result.0.insert(id, field).is_some() {
                return Err(serde::de::Error::custom("duplicate standard entry field"));
            }
        }
        result.ensure_standard_fields();
        Ok(result)
    }
}

impl EntryFields {
    fn with_defaults() -> Self {
        let mut fields = Self::default();
        fields.ensure_standard_fields();
        fields
    }

    fn ensure_standard_fields(&mut self) {
        for standard in StandardField::ALL {
            self.0
                .entry(EntryFieldId::Standard(standard))
                .or_insert_with(|| EntryField::new(standard.name(), standard.default_value()));
        }
    }
}

/// A KeePass database entry (password record).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    /// Unique identifier
    pub id: NodeId,
    pub(crate) fields: EntryFields,
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

/// A string field in an entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntryField {
    pub(crate) name: String,
    pub(crate) value: ProtectedString,
    #[serde(skip)]
    pub(crate) xml_extensions: Vec<PreservedXmlElement>,
}

impl EntryField {
    pub(crate) fn new(name: impl Into<String>, value: ProtectedString) -> Self {
        Self {
            name: name.into(),
            value,
            xml_extensions: Vec::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &ProtectedString {
        &self.value
    }

    pub fn standard(&self) -> Option<StandardField> {
        StandardField::from_name(&self.name)
    }
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
            fields: EntryFields::with_defaults(),
            icon: IconImage::default(),
            custom_icon_uuid: None,
            background_color: String::new(),
            foreground_color: String::new(),
            override_url: String::new(),
            tags: Vec::new(),
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

    pub fn fields(&self) -> impl ExactSizeIterator<Item = (EntryFieldId, &EntryField)> {
        self.fields.0.iter().map(|(id, field)| (*id, field))
    }

    pub fn field(&self, id: EntryFieldId) -> Option<&EntryField> {
        self.fields.0.get(&id)
    }

    pub fn title(&self) -> &ProtectedString {
        self.standard_value(StandardField::Title)
    }

    pub fn set_title(&mut self, value: impl Into<ProtectedString>) {
        self.set_standard_value(StandardField::Title, value.into());
    }

    pub fn username(&self) -> &ProtectedString {
        self.standard_value(StandardField::UserName)
    }

    pub fn set_username(&mut self, value: impl Into<ProtectedString>) {
        self.set_standard_value(StandardField::UserName, value.into());
    }

    pub fn password(&self) -> &ProtectedString {
        self.standard_value(StandardField::Password)
    }

    pub fn set_password(&mut self, value: impl Into<ProtectedString>) {
        self.set_standard_value(StandardField::Password, value.into());
    }

    pub fn url(&self) -> &ProtectedString {
        self.standard_value(StandardField::Url)
    }

    pub fn set_url(&mut self, value: impl Into<ProtectedString>) {
        self.set_standard_value(StandardField::Url, value.into());
    }

    pub fn notes(&self) -> &ProtectedString {
        self.standard_value(StandardField::Notes)
    }

    pub fn set_notes(&mut self, value: impl Into<ProtectedString>) {
        self.set_standard_value(StandardField::Notes, value.into());
    }

    pub fn custom_fields(&self) -> impl Iterator<Item = (EntryFieldId, &EntryField)> {
        self.fields()
            .filter(|(id, _)| matches!(id, EntryFieldId::Custom(_)))
    }

    /// Whether this entry has exactly one unprotected `_etm_template=1` marker.
    pub fn is_etm_template(&self) -> bool {
        let mut markers = self
            .custom_fields()
            .filter(|(_, field)| field.name == ETM_TEMPLATE);
        let Some((_, marker)) = markers.next() else {
            return false;
        };
        markers.next().is_none() && !marker.value.is_protected() && marker.value.as_str() == "1"
    }

    /// UUID of this entry's ETM template, when the link is unique and unprotected.
    pub fn etm_template_uuid(&self) -> Option<Uuid> {
        parse_template_uuid(self)
    }

    pub fn add_custom_field(
        &mut self,
        name: impl Into<String>,
        value: ProtectedString,
    ) -> EntryFieldId {
        let name = name.into();
        assert!(StandardField::from_name(&name).is_none());
        let id = EntryFieldId::Custom(Uuid::new_v4());
        self.fields.0.insert(id, EntryField::new(name, value));
        id
    }

    pub(crate) fn retain_custom_fields(&mut self, keep: impl Fn(&EntryField) -> bool) {
        self.fields
            .0
            .retain(|id, field| matches!(id, EntryFieldId::Standard(_)) || keep(field));
    }

    fn standard_value(&self, standard: StandardField) -> &ProtectedString {
        &self
            .fields
            .0
            .get(&EntryFieldId::Standard(standard))
            .expect("standard entry fields are always present")
            .value
    }

    fn set_standard_value(&mut self, standard: StandardField, value: ProtectedString) {
        self.fields
            .0
            .get_mut(&EntryFieldId::Standard(standard))
            .expect("standard entry fields are always present")
            .value = value;
    }

    pub(crate) fn begin_field_import(&mut self) {
        self.fields.0.clear();
    }

    pub(crate) fn add_imported_field(
        &mut self,
        name: String,
        value: ProtectedString,
        xml_extensions: Vec<PreservedXmlElement>,
    ) -> DatabaseResult<()> {
        let id = StandardField::from_name(&name)
            .map(EntryFieldId::Standard)
            .unwrap_or_else(|| EntryFieldId::Custom(Uuid::new_v4()));
        let field = EntryField {
            name,
            value,
            xml_extensions,
        };
        if self.fields.0.insert(id, field).is_some() {
            return Err(DatabaseError::InvalidFormat(
                "duplicate standard entry field".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn finish_field_import(&mut self) {
        self.fields.ensure_standard_fields();
    }

    pub(crate) fn fields_mut(&mut self) -> impl Iterator<Item = (EntryFieldId, &mut EntryField)> {
        self.fields.0.iter_mut().map(|(id, field)| (*id, field))
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
        self.field_for_selector(selector)?
            .value
            .as_unsealed_str()
            .map(use_value)
    }

    fn field_for_selector(&self, selector: &EntryFieldSelector) -> Option<&EntryField> {
        match selector {
            EntryFieldSelector::Title => self
                .fields
                .0
                .get(&EntryFieldId::Standard(StandardField::Title)),
            EntryFieldSelector::UserName => self
                .fields
                .0
                .get(&EntryFieldId::Standard(StandardField::UserName)),
            EntryFieldSelector::Password => self
                .fields
                .0
                .get(&EntryFieldId::Standard(StandardField::Password)),
            EntryFieldSelector::Url => self
                .fields
                .0
                .get(&EntryFieldId::Standard(StandardField::Url)),
            EntryFieldSelector::Notes => self
                .fields
                .0
                .get(&EntryFieldId::Standard(StandardField::Notes)),
            EntryFieldSelector::Custom(name) => self
                .custom_fields()
                .find(|(_, field)| field.name == *name)
                .map(|(_, field)| field),
        }
    }

    pub(crate) fn seal_protected_strings(
        &mut self,
        context: std::sync::Arc<MemoryProtectionContext>,
        root: &[u8; 32],
    ) -> DatabaseResult<()> {
        let entry_id = self.id;
        for (id, field) in self.fields_mut() {
            let memory_field = memory_field(id, &field.name);
            field
                .value
                .seal(context.clone(), root, entry_id, &memory_field)?;
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
        let value = self
            .field_for_memory(field)
            .ok_or_else(|| DatabaseError::InvalidFormat("unknown entry field".into()))?;
        value
            .value
            .with_plaintext(unlock, self.id, field, use_value)
    }

    pub(crate) fn replace_memory_field(
        &mut self,
        context: std::sync::Arc<MemoryProtectionContext>,
        root: &[u8; 32],
        field: &MemoryField,
        value: &str,
        protected: bool,
    ) -> DatabaseResult<()> {
        let entry_id = self.id;
        let target = self
            .field_for_memory_mut(field)
            .ok_or_else(|| DatabaseError::InvalidFormat("unknown entry field".into()))?;
        replace_protected_string(
            &mut target.value,
            context,
            root,
            entry_id,
            field,
            value,
            protected,
        )?;
        Ok(())
    }

    fn field_for_memory(&self, field: &MemoryField) -> Option<&EntryField> {
        match field {
            MemoryField::Title => self
                .fields
                .0
                .get(&EntryFieldId::Standard(StandardField::Title)),
            MemoryField::UserName => self
                .fields
                .0
                .get(&EntryFieldId::Standard(StandardField::UserName)),
            MemoryField::Password => self
                .fields
                .0
                .get(&EntryFieldId::Standard(StandardField::Password)),
            MemoryField::Url => self
                .fields
                .0
                .get(&EntryFieldId::Standard(StandardField::Url)),
            MemoryField::Notes => self
                .fields
                .0
                .get(&EntryFieldId::Standard(StandardField::Notes)),
            MemoryField::Custom(name) => self
                .custom_fields()
                .find(|(_, candidate)| candidate.name == *name)
                .map(|(_, field)| field),
        }
    }

    fn field_for_memory_mut(&mut self, field: &MemoryField) -> Option<&mut EntryField> {
        let id = match field {
            MemoryField::Title => EntryFieldId::Standard(StandardField::Title),
            MemoryField::UserName => EntryFieldId::Standard(StandardField::UserName),
            MemoryField::Password => EntryFieldId::Standard(StandardField::Password),
            MemoryField::Url => EntryFieldId::Standard(StandardField::Url),
            MemoryField::Notes => EntryFieldId::Standard(StandardField::Notes),
            MemoryField::Custom(name) => self
                .fields
                .0
                .iter()
                .find(|(id, candidate)| {
                    matches!(id, EntryFieldId::Custom(_)) && candidate.name == *name
                })
                .map(|(id, _)| *id)?,
        };
        self.fields.0.get_mut(&id)
    }

    pub(crate) fn semantic_clone(
        &self,
        unlock: &mut MemoryUnlockSession<'_>,
    ) -> DatabaseResult<Self> {
        let mut clone = self.clone();
        for (id, source) in self.fields() {
            let field = memory_field(id, &source.name);
            let value = source
                .value
                .with_plaintext(unlock, self.id, &field, |value| Ok(value.to_string()))?;
            let target = clone.fields.0.get_mut(&id).expect("cloned field");
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
        for (id, field) in self.fields_mut() {
            let memory_field = memory_field(id, &field.name);
            field
                .value
                .rebind(unlock, old_entry_id, new_entry_id, &memory_field)?;
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
            for (_, field) in self.fields_mut() {
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

pub(crate) fn memory_field(id: EntryFieldId, name: &str) -> MemoryField {
    match id {
        EntryFieldId::Standard(StandardField::Title) => MemoryField::Title,
        EntryFieldId::Standard(StandardField::UserName) => MemoryField::UserName,
        EntryFieldId::Standard(StandardField::Password) => MemoryField::Password,
        EntryFieldId::Standard(StandardField::Url) => MemoryField::Url,
        EntryFieldId::Standard(StandardField::Notes) => MemoryField::Notes,
        EntryFieldId::Custom(_) => MemoryField::Custom(name.to_string()),
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
        self.title().as_unsealed_str().unwrap_or("")
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
        assert!(entry.title().is_empty());
        assert!(entry.tags.is_empty());
        assert!(!entry.expires);
    }

    #[test]
    fn test_entry_push_history() {
        let mut entry = Entry::new(NodeId::new_uuid());
        entry.set_title("V1");

        entry.push_history();
        entry.set_title("V2");

        assert_eq!(entry.history_count(), 1);
        assert_eq!(entry.history[0].title(), "V1");
        assert_eq!(entry.title(), "V2");
    }

    #[test]
    fn test_entry_restore_from_history() {
        let mut entry = Entry::new(NodeId::new_uuid());
        let id = entry.id;
        entry.set_title("Original");

        entry.push_history();
        entry.set_title("Modified");

        assert_eq!(entry.title(), "Modified");
        assert!(entry.restore_from_history(0));
        assert_eq!(entry.title(), "Original");
        assert_eq!(entry.id, id); // ID preserved
    }

    #[test]
    fn test_entry_history_limit() {
        let mut entry = Entry::new(NodeId::new_uuid());
        for i in 0..15 {
            entry.push_history();
            entry.set_title(format!("V{}", i));
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
