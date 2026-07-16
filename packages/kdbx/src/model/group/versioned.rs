//! Versioned Group adapters
//!
//! Handles mapping between KDB (v1) and KDBX (v3.1/v4) group formats.

use crate::model::core::date::DateInstant;
use crate::model::core::node::NodeId;
use crate::model::group::Group;
use crate::model::meta::icon::IconImage;

/// KDB (v1) specific group fields
pub mod kdb_group_field {
    pub const GROUP_ID: u16 = 0x0001;
    pub const TITLE: u16 = 0x0002;
    pub const CREATION_TIME: u16 = 0x0003;
    pub const LAST_MODIFICATION_TIME: u16 = 0x0004;
    pub const LAST_ACCESS_TIME: u16 = 0x0005;
    pub const EXPIRY_TIME: u16 = 0x0006;
    pub const ICON_ID: u16 = 0x0007;
    pub const LEVEL: u16 = 0x0008;
    pub const FLAGS: u16 = 0x0009;
    pub const END: u16 = 0xFFFF;
}

/// KDB group flags
pub mod kdb_group_flag {
    pub const NONE: u32 = 0;
    pub const EXPANDED: u32 = 1;
    pub const SEARCH_ENABLED: u32 = 2;
    pub const AUTO_TYPE_NAVIGATING: u32 = 4;
}

/// Adapter for converting between KDB and unified Group format.
pub struct GroupKDB;

impl GroupKDB {
    /// Create a Group from KDB raw field data.
    pub fn from_kdb_fields(fields: &std::collections::HashMap<u16, Vec<u8>>) -> Group {
        let group_id = fields
            .get(&kdb_group_field::GROUP_ID)
            .and_then(|v| {
                v.get(..4)
                    .map(|s| u32::from_le_bytes(s.try_into().unwrap_or([0; 4])))
            })
            .unwrap_or(0);

        let id = NodeId::from_u32(group_id);
        let mut g = Group::new(id);

        g.title = fields
            .get(&kdb_group_field::TITLE)
            .map(|v| String::from_utf8_lossy(v).to_string())
            .unwrap_or_default();

        if let Some(data) = fields.get(&kdb_group_field::ICON_ID) {
            if data.len() >= 4 {
                let icon_id = u32::from_le_bytes(data[..4].try_into().unwrap_or([0; 4]));
                g.icon =
                    IconImage::Standard(crate::model::meta::icon::IconImageStandard::new(icon_id));
            }
        }

        if let Some(data) = fields.get(&kdb_group_field::CREATION_TIME) {
            g.creation_time = DateInstant::EpochMillis(read_i64_le(data) * 1000);
        }
        if let Some(data) = fields.get(&kdb_group_field::LAST_MODIFICATION_TIME) {
            g.last_modification_time = DateInstant::EpochMillis(read_i64_le(data) * 1000);
        }
        if let Some(data) = fields.get(&kdb_group_field::FLAGS) {
            if data.len() >= 4 {
                let flags = u32::from_le_bytes(data[..4].try_into().unwrap_or([0; 4]));
                g.is_expanded = (flags & kdb_group_flag::EXPANDED) != 0;
                g.enable_searching = (flags & kdb_group_flag::SEARCH_ENABLED) != 0;
                g.is_autotype_navigating = (flags & kdb_group_flag::AUTO_TYPE_NAVIGATING) != 0;
            }
        }

        g
    }

    /// Convert a Group to KDB raw field data.
    pub fn to_kdb_fields(group: &Group) -> Vec<(u16, Vec<u8>)> {
        let mut fields = Vec::new();

        // Group ID (use u32 representation)
        if let Some(id_u32) = group.id.as_u32() {
            fields.push((kdb_group_field::GROUP_ID, id_u32.to_le_bytes().to_vec()));
        }

        fields.push((kdb_group_field::TITLE, group.title.as_bytes().to_vec()));

        let icon_id = match &group.icon {
            IconImage::Standard(s) => s.icon_id,
            _ => 0,
        };
        fields.push((kdb_group_field::ICON_ID, icon_id.to_le_bytes().to_vec()));

        fields.push((
            kdb_group_field::CREATION_TIME,
            (group.creation_time.as_millis().unwrap_or(0) / 1000)
                .to_le_bytes()
                .to_vec(),
        ));
        fields.push((
            kdb_group_field::LAST_MODIFICATION_TIME,
            (group.last_modification_time.as_millis().unwrap_or(0) / 1000)
                .to_le_bytes()
                .to_vec(),
        ));

        // Flags
        let mut flags: u32 = 0;
        if group.is_expanded {
            flags |= kdb_group_flag::EXPANDED;
        }
        if group.enable_searching {
            flags |= kdb_group_flag::SEARCH_ENABLED;
        }
        if group.is_autotype_navigating {
            flags |= kdb_group_flag::AUTO_TYPE_NAVIGATING;
        }
        fields.push((kdb_group_field::FLAGS, flags.to_le_bytes().to_vec()));

        fields.push((kdb_group_field::END, Vec::new()));
        fields
    }
}

/// Adapter for KDBX group format.
pub struct GroupKDBX;

impl GroupKDBX {
    /// Get the default auto-type sequence for a group.
    /// Empty string means inherit from parent.
    pub fn default_auto_type_sequence(group: &Group) -> &str {
        &group.default_autotype_sequence
    }

    /// Check if a group is a system group (should be hidden from normal view).
    pub fn is_system_group(group: &Group) -> bool {
        group.title == "Backup" || group.title == "Meta-System"
    }
}

fn read_i64_le(data: &[u8]) -> i64 {
    if data.len() >= 8 {
        let bytes: [u8; 8] = data[..8].try_into().unwrap_or([0; 8]);
        i64::from_le_bytes(bytes)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_kdb_group_from_fields() {
        let mut fields = HashMap::new();
        fields.insert(kdb_group_field::GROUP_ID, 42u32.to_le_bytes().to_vec());
        fields.insert(kdb_group_field::TITLE, b"My Group".to_vec());
        fields.insert(kdb_group_field::ICON_ID, 10u32.to_le_bytes().to_vec());
        fields.insert(
            kdb_group_field::FLAGS,
            (kdb_group_flag::EXPANDED | kdb_group_flag::SEARCH_ENABLED)
                .to_le_bytes()
                .to_vec(),
        );

        let group = GroupKDB::from_kdb_fields(&fields);
        assert_eq!(group.title, "My Group");
        assert!(group.is_expanded);
        assert!(group.enable_searching);
    }

    #[test]
    fn test_kdb_group_to_fields() {
        let id = NodeId::from_u32(5);
        let mut group = Group::new(id);
        group.title = "Test".to_string();
        group.is_expanded = false;

        let fields = GroupKDB::to_kdb_fields(&group);
        let map: HashMap<u16, Vec<u8>> = fields.into_iter().collect();

        assert_eq!(
            String::from_utf8_lossy(map.get(&kdb_group_field::TITLE).unwrap()),
            "Test"
        );
    }

    #[test]
    fn test_roundtrip_kdb_group() {
        let id = NodeId::from_u32(99);
        let mut g = Group::new(id);
        g.title = "RoundTrip".to_string();
        g.is_expanded = true;
        g.enable_searching = false;

        let fields = GroupKDB::to_kdb_fields(&g);
        let map: HashMap<u16, Vec<u8>> = fields.into_iter().collect();
        let restored = GroupKDB::from_kdb_fields(&map);

        assert_eq!(restored.title, "RoundTrip");
        assert!(restored.is_expanded);
        assert!(!restored.enable_searching);
    }

    #[test]
    fn test_kdbx_system_group() {
        let mut g = Group::new(NodeId::new_uuid());
        g.title = "Backup".to_string();
        assert!(GroupKDBX::is_system_group(&g));

        g.title = "Normal".to_string();
        assert!(!GroupKDBX::is_system_group(&g));
    }
}
