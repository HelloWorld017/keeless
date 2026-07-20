//! Versioned Entry adapters
//!
//! Handles mapping between KDB (v1) and KDBX (v3.1/v4) entry formats.

use crate::crypto::memory_protection::{MemoryField, MemoryUnlockSession};
use crate::model::core::date::DateInstant;
use crate::model::core::node::NodeId;
use crate::model::core::security::ProtectedString;
use crate::model::entry::Entry;
use crate::model::exception::DatabaseResult;
use crate::model::meta::icon::{IconImage, IconImageStandard};

/// KDB (v1) specific entry fields
/// KDB format uses numeric field IDs instead of string keys.
pub mod kdb_field {
    pub const IGNORE: u16 = 0x0000;
    pub const GROUP_ID: u16 = 0x0001;
    pub const ICON_ID: u16 = 0x0002;
    pub const TITLE: u16 = 0x0003;
    pub const USER_NAME: u16 = 0x0004;
    pub const PASSWORD: u16 = 0x0005;
    pub const NOTES: u16 = 0x0006;
    pub const CREATION_TIME: u16 = 0x0007;
    pub const LAST_MODIFICATION_TIME: u16 = 0x0008;
    pub const LAST_ACCESS_TIME: u16 = 0x0009;
    pub const EXPIRY_TIME: u16 = 0x000A;
    pub const BINARY_DESC: u16 = 0x000B;
    pub const ATTACHMENT: u16 = 0x000C;
    pub const END: u16 = 0xFFFF;
}

/// Adapter for converting between KDB and unified Entry format.
pub struct EntryKDB;

impl EntryKDB {
    /// Create an Entry from KDB raw field data.
    pub fn from_kdb_fields(id: NodeId, fields: &std::collections::HashMap<u16, Vec<u8>>) -> Entry {
        let mut e = Entry::new(id);

        e.title = ProtectedString::new_plain(&get_string(fields, kdb_field::TITLE));
        e.username = ProtectedString::new_plain(&get_string(fields, kdb_field::USER_NAME));
        e.password = ProtectedString::new_protected(&get_string(fields, kdb_field::PASSWORD));
        e.notes = ProtectedString::new_plain(&get_string(fields, kdb_field::NOTES));

        if let Some(data) = fields.get(&kdb_field::ICON_ID) {
            if data.len() >= 4 {
                let icon_id = u32::from_le_bytes(data[..4].try_into().unwrap_or([0; 4]));
                e.icon = IconImage::Standard(IconImageStandard::new(icon_id));
            }
        }

        if let Some(data) = fields.get(&kdb_field::CREATION_TIME) {
            e.creation_time = DateInstant::EpochMillis(read_u64_le(data) * 1000);
        }
        if let Some(data) = fields.get(&kdb_field::LAST_MODIFICATION_TIME) {
            e.last_modification_time = DateInstant::EpochMillis(read_u64_le(data) * 1000);
        }
        if let Some(data) = fields.get(&kdb_field::LAST_ACCESS_TIME) {
            e.last_access_time = DateInstant::EpochMillis(read_u64_le(data) * 1000);
        }
        if let Some(data) = fields.get(&kdb_field::EXPIRY_TIME) {
            e.expiry_time = DateInstant::EpochMillis(read_u64_le(data) * 1000);
        }

        // Binary attachment
        if let Some(data) = fields.get(&kdb_field::ATTACHMENT) {
            let desc = get_string(fields, kdb_field::BINARY_DESC);
            e.binaries.push(super::EntryBinary {
                name: desc,
                data: data.clone(),
                is_protected: false,
            });
        }

        e
    }

    /// Convert an Entry to KDB raw field data.
    pub fn to_kdb_fields(entry: &Entry) -> Vec<(u16, Vec<u8>)> {
        let icon_id = match &entry.icon {
            IconImage::Standard(s) => s.icon_id,
            _ => 0,
        };
        let mut fields = vec![
            (kdb_field::TITLE, entry.title.as_bytes().to_vec()),
            (
                kdb_field::USER_NAME,
                entry.username.as_str().as_bytes().to_vec(),
            ),
            (
                kdb_field::PASSWORD,
                entry.password.as_str().as_bytes().to_vec(),
            ),
            (kdb_field::NOTES, entry.notes.as_str().as_bytes().to_vec()),
            (kdb_field::ICON_ID, icon_id.to_le_bytes().to_vec()),
            (
                kdb_field::CREATION_TIME,
                (entry.creation_time.as_millis().unwrap_or(0) / 1000)
                    .to_le_bytes()
                    .to_vec(),
            ),
            (
                kdb_field::LAST_MODIFICATION_TIME,
                (entry.last_modification_time.as_millis().unwrap_or(0) / 1000)
                    .to_le_bytes()
                    .to_vec(),
            ),
            (
                kdb_field::LAST_ACCESS_TIME,
                (entry.last_access_time.as_millis().unwrap_or(0) / 1000)
                    .to_le_bytes()
                    .to_vec(),
            ),
            (
                kdb_field::EXPIRY_TIME,
                (entry.expiry_time.as_millis().unwrap_or(0) / 1000)
                    .to_le_bytes()
                    .to_vec(),
            ),
        ];

        // First binary attachment only (KDB v1 supports only one)
        if let Some(binary) = entry.binaries.first() {
            fields.push((kdb_field::BINARY_DESC, binary.name.as_bytes().to_vec()));
            fields.push((kdb_field::ATTACHMENT, binary.data.clone()));
        }

        fields.push((kdb_field::END, Vec::new()));
        fields
    }

    pub(crate) fn to_kdb_fields_with_memory(
        entry: &Entry,
        unlock: &mut MemoryUnlockSession<'_>,
    ) -> DatabaseResult<Vec<(u16, Vec<u8>)>> {
        let icon_id = match &entry.icon {
            IconImage::Standard(icon) => icon.icon_id,
            _ => 0,
        };
        let mut fields = Vec::new();
        for (field_id, memory_field) in [
            (kdb_field::TITLE, MemoryField::Title),
            (kdb_field::USER_NAME, MemoryField::UserName),
            (kdb_field::PASSWORD, MemoryField::Password),
            (kdb_field::NOTES, MemoryField::Notes),
        ] {
            entry.with_memory_field(unlock, &memory_field, |value| {
                fields.push((field_id, value.as_bytes().to_vec()));
                Ok(())
            })?;
        }
        fields.extend([
            (kdb_field::ICON_ID, icon_id.to_le_bytes().to_vec()),
            (
                kdb_field::CREATION_TIME,
                (entry.creation_time.as_millis().unwrap_or(0) / 1000)
                    .to_le_bytes()
                    .to_vec(),
            ),
            (
                kdb_field::LAST_MODIFICATION_TIME,
                (entry.last_modification_time.as_millis().unwrap_or(0) / 1000)
                    .to_le_bytes()
                    .to_vec(),
            ),
            (
                kdb_field::LAST_ACCESS_TIME,
                (entry.last_access_time.as_millis().unwrap_or(0) / 1000)
                    .to_le_bytes()
                    .to_vec(),
            ),
            (
                kdb_field::EXPIRY_TIME,
                (entry.expiry_time.as_millis().unwrap_or(0) / 1000)
                    .to_le_bytes()
                    .to_vec(),
            ),
        ]);
        if let Some(binary) = entry.binaries.first() {
            fields.push((kdb_field::BINARY_DESC, binary.name.as_bytes().to_vec()));
            fields.push((kdb_field::ATTACHMENT, binary.data.clone()));
        }
        fields.push((kdb_field::END, Vec::new()));
        Ok(fields)
    }
}

/// Adapter for KDBX entry format.
/// KDBX uses XML string fields, so most mapping is done in the XML layer.
pub struct EntryKDBX;

impl EntryKDBX {
    /// Get the template type from an entry's custom fields.
    /// This is used to determine what template icon/metadata to show.
    pub fn get_template_type(entry: &Entry) -> Option<String> {
        // Check if the entry has a "_etm_type" custom field ( KeePass template marker)
        for f in &entry.custom_fields {
            if f.name == "KeePassFieldType" || f.name == "_etm_type" {
                return Some(f.value.as_str().to_string());
            }
        }
        None
    }

    /// Check if entry should be serialized with protected fields.
    /// In KDBX, the inner stream cipher handles protection.
    pub fn should_protect_field(field_name: &str) -> bool {
        matches!(field_name, "Password")
    }

    /// Get entry field names in standard KDBX order.
    pub fn field_order() -> &'static [&'static str] {
        &["Title", "UserName", "Password", "URL", "Notes"]
    }
}

fn get_string(fields: &std::collections::HashMap<u16, Vec<u8>>, key: u16) -> String {
    fields
        .get(&key)
        .map(|v| String::from_utf8_lossy(v).to_string())
        .unwrap_or_default()
}

fn read_u64_le(data: &[u8]) -> i64 {
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
    use crate::model::entry::EntryBinary;
    use std::collections::HashMap;

    #[test]
    fn test_kdb_entry_from_fields() {
        let mut fields = HashMap::new();
        fields.insert(kdb_field::TITLE, b"Test Entry".to_vec());
        fields.insert(kdb_field::USER_NAME, b"user123".to_vec());
        fields.insert(kdb_field::PASSWORD, b"secret".to_vec());
        fields.insert(kdb_field::NOTES, b"Some notes".to_vec());
        fields.insert(kdb_field::ICON_ID, 5u32.to_le_bytes().to_vec());

        let entry = EntryKDB::from_kdb_fields(NodeId::new_uuid(), &fields);
        assert_eq!(entry.title, "Test Entry");
        assert_eq!(entry.username.as_str(), "user123");
        assert_eq!(entry.password.as_str(), "secret");
    }

    #[test]
    fn test_kdb_entry_to_fields() {
        let mut entry = Entry::new(NodeId::new_uuid());
        entry.title = "Test".into();
        entry.username = ProtectedString::new_plain("user");
        entry.password = ProtectedString::new_protected("pass");

        let fields = EntryKDB::to_kdb_fields(&entry);
        assert!(fields.iter().any(|(k, _)| *k == kdb_field::TITLE));
        assert!(fields.iter().any(|(k, _)| *k == kdb_field::PASSWORD));
        assert!(fields.iter().any(|(k, _)| *k == kdb_field::END));
    }

    #[test]
    fn test_kdbx_field_order() {
        let order = EntryKDBX::field_order();
        assert_eq!(order[0], "Title");
        assert_eq!(order[1], "UserName");
        assert_eq!(order[2], "Password");
    }

    #[test]
    fn test_kdbx_should_protect() {
        assert!(EntryKDBX::should_protect_field("Password"));
        assert!(!EntryKDBX::should_protect_field("Title"));
        assert!(!EntryKDBX::should_protect_field("UserName"));
    }

    #[test]
    fn test_roundtrip_kdb() {
        let id = NodeId::new_uuid();
        let mut entry = Entry::new(id);
        entry.title = "RoundTrip".into();
        entry.username = ProtectedString::new_plain("u");
        entry.password = ProtectedString::new_protected("p");
        entry.notes = ProtectedString::new_plain("n");
        entry.binaries.push(EntryBinary {
            name: "file.txt".to_string(),
            data: vec![0x42, 0x43],
            is_protected: false,
        });

        let fields = EntryKDB::to_kdb_fields(&entry);
        let map: HashMap<u16, Vec<u8>> = fields.into_iter().collect();
        let restored = EntryKDB::from_kdb_fields(id, &map);

        assert_eq!(restored.title, "RoundTrip");
        assert_eq!(restored.username.as_str(), "u");
        assert_eq!(restored.binaries.len(), 1);
        assert_eq!(restored.binaries[0].name, "file.txt");
    }
}
