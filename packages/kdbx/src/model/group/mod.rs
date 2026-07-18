//! Group data model
//!

pub mod versioned;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::core::date::DateInstant;
use crate::model::core::node::{Node, NodeId, NodeType};
use crate::model::meta::custom_data::CustomData;
use crate::model::meta::icon::IconImage;
use crate::model::xml::GroupXmlExtensions;

/// A KeePass database group (folder).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Group {
    /// Unique identifier
    pub id: NodeId,
    /// Group name
    pub title: String,
    /// Icon
    pub icon: IconImage,
    /// Custom icon UUID
    pub custom_icon_uuid: Option<Uuid>,
    /// Notes
    pub notes: String,
    /// Child groups (indices into the database's group list)
    pub child_group_ids: Vec<NodeId>,
    /// Child entries (indices into the database's entry list)
    pub child_entry_ids: Vec<NodeId>,
    /// Is this group expanded?
    pub is_expanded: bool,
    /// Creation time
    pub creation_time: DateInstant,
    /// Last modification time
    pub last_modification_time: DateInstant,
    /// Last access time.
    pub last_access_time: DateInstant,
    /// Last location change time.
    pub location_changed: DateInstant,
    /// Expiry time
    pub expiry_time: DateInstant,
    /// Whether the group expires
    pub expires: bool,
    /// Usage count
    pub usage_count: i64,
    /// Enable searching
    pub enable_searching: bool,
    /// Is this an auto-type navigating group?
    pub is_autotype_navigating: bool,
    /// Default auto-type sequence
    pub default_autotype_sequence: String,
    /// Extensible KDBX custom data.
    pub custom_data: CustomData,
    /// Opaque XML elements retained for forward-compatible round-trips.
    #[doc(hidden)]
    #[serde(skip)]
    pub xml_extensions: GroupXmlExtensions,
}

impl Group {
    pub fn new(id: NodeId) -> Self {
        let now = DateInstant::now();
        Self {
            id,
            title: String::new(),
            icon: IconImage::default(),
            custom_icon_uuid: None,
            notes: String::new(),
            child_group_ids: Vec::new(),
            child_entry_ids: Vec::new(),
            is_expanded: true,
            creation_time: now,
            last_modification_time: now,
            last_access_time: now,
            location_changed: now,
            expiry_time: DateInstant::never(),
            expires: false,
            usage_count: 0,
            enable_searching: true,
            is_autotype_navigating: false,
            default_autotype_sequence: String::new(),
            custom_data: CustomData::default(),
            xml_extensions: GroupXmlExtensions::default(),
        }
    }

    /// Add a child group ID
    pub fn add_child_group(&mut self, group_id: NodeId) {
        if !self.child_group_ids.contains(&group_id) {
            self.child_group_ids.push(group_id);
        }
    }

    /// Add a child entry ID
    pub fn add_child_entry(&mut self, entry_id: NodeId) {
        if !self.child_entry_ids.contains(&entry_id) {
            self.child_entry_ids.push(entry_id);
        }
    }
}

impl Node for Group {
    fn node_id(&self) -> &NodeId {
        &self.id
    }

    fn node_type(&self) -> NodeType {
        NodeType::Group
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
    fn test_group_creation() {
        let group = Group::new(NodeId::new_uuid());
        assert!(group.title.is_empty());
        assert!(group.is_expanded);
        assert!(group.child_group_ids.is_empty());
    }

    #[test]
    fn test_group_add_children() {
        let mut group = Group::new(NodeId::new_uuid());
        let child_id = NodeId::new_uuid();
        let entry_id = NodeId::new_uuid();

        group.add_child_group(child_id);
        group.add_child_entry(entry_id);

        assert_eq!(group.child_group_ids.len(), 1);
        assert_eq!(group.child_entry_ids.len(), 1);

        // Adding duplicate should not increase count
        group.add_child_group(child_id);
        assert_eq!(group.child_group_ids.len(), 1);
    }
}
