//! KDBX XML serialization / deserialization
//!
//! Handles the inner XML structure of KDBX databases.

mod helpers;
mod reader;
mod writer;

pub use reader::KdbxXmlReader;
pub use writer::KdbxXmlWriter;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::inner_stream::Salsa20InnerStream;
    use crate::model::core::node::NodeId;
    use crate::model::core::security::ProtectedString;
    use crate::model::db::database::Database;
    use crate::model::entry::Entry;
    use crate::model::group::Group;
    use uuid::Uuid;

    fn make_test_db() -> Database {
        let mut db = Database {
            name: "TestDB".to_string(),
            ..Database::default()
        };

        let root_id =
            NodeId::from_uuid(Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567890").unwrap());
        let mut root = Group::new(root_id);
        root.title = "Root".to_string();

        let entry_id =
            NodeId::from_uuid(Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap());
        let mut entry = Entry::new(entry_id);
        entry.title = "Test Entry".to_string();
        entry.username = ProtectedString::new_plain("user@test.com");
        entry.password = ProtectedString::new_protected("s3cret!");
        entry.url = "https://example.com".to_string();

        root.add_child_entry(entry_id);
        db.entries.insert(entry_id, entry);
        db.groups.insert(root_id, root);
        db.root_group_id = Some(root_id);
        db
    }

    #[test]
    fn test_xml_roundtrip() {
        let db = make_test_db();

        // Write
        let key = b"test_xml_key_12345";
        let mut is_write = Salsa20InnerStream::new(key);
        let xml = KdbxXmlWriter::write(&db, &mut is_write).unwrap();
        assert!(xml.contains("<KeePassFile>"));
        assert!(xml.contains("<DatabaseName>TestDB</DatabaseName>"));

        // Read
        let mut is_read = Salsa20InnerStream::new(key);
        let db2 = KdbxXmlReader::read(&xml, &mut is_read).unwrap();
        assert_eq!(db2.name, "TestDB");
        assert_eq!(db2.entries.len(), 1);
        assert_eq!(db2.entries.len(), 1);
    }

    #[test]
    fn test_protected_field_roundtrip() {
        let db = make_test_db();
        let key = b"key_for_protected";

        let mut is_w = Salsa20InnerStream::new(key);
        let xml = KdbxXmlWriter::write(&db, &mut is_w).unwrap();

        // Verify Password is base64-encoded (not plaintext)
        assert!(!xml.contains("s3cret!"));
        assert!(xml.contains("Protected=\"True\""));
        assert!(!xml.contains("ProtectInMemory"));

        // Read back
        let mut is_r = Salsa20InnerStream::new(key);
        let db2 = KdbxXmlReader::read(&xml, &mut is_r).unwrap();

        let entry = db2.entries.values().next().unwrap();
        assert_eq!(entry.password.as_str(), "s3cret!");
        assert!(entry.password.is_protected());
        assert!(!entry.username.is_protected());
    }

    #[test]
    fn test_invalid_protected_value_is_format_error() {
        let xml = r#"<KeePassFile><Root><Group><UUID>obLD1OX2eJCrze8SNFZ4kA</UUID><Name>Root</Name><Entry><UUID>ERERESIiMzNERFVVVVVVVQ</UUID><String><Key>Password</Key><Value Protected="True">not-base64!</Value></String></Entry></Group></Root></KeePassFile>"#;
        let mut stream = Salsa20InnerStream::new(b"invalid-value");
        assert!(matches!(
            KdbxXmlReader::read(xml, &mut stream),
            Err(crate::DatabaseError::InvalidFormat(_))
        ));
    }

    #[test]
    fn test_deleted_object_time_roundtrip() {
        let mut db = make_test_db();
        let deleted_id = NodeId::new_uuid();
        db.deleted_objects.push(crate::model::DeletedObject {
            id: deleted_id,
            deletion_time: 1_725_000_123_000,
        });
        let key = b"deleted-time";
        let mut write_stream = Salsa20InnerStream::new(key);
        let xml = KdbxXmlWriter::write(&db, &mut write_stream).unwrap();
        let mut read_stream = Salsa20InnerStream::new(key);
        let loaded = KdbxXmlReader::read(&xml, &mut read_stream).unwrap();
        assert_eq!(loaded.deleted_objects[0].id, deleted_id);
        assert_eq!(loaded.deleted_objects[0].deletion_time, 1_725_000_123_000);
    }

    #[test]
    fn test_unknown_xml_refuses_save() {
        let xml = r#"<KeePassFile><Root><Group><UUID>obLD1OX2eJCrze8SNFZ4kA</UUID><Name>Root</Name><FutureElement><Value>x</Value></FutureElement></Group></Root></KeePassFile>"#;
        let key = b"unsupported-xml";
        let mut read_stream = Salsa20InnerStream::new(key);
        let db = KdbxXmlReader::read(xml, &mut read_stream).unwrap();
        assert!(db.contains_unsupported_xml);
        let mut write_stream = Salsa20InnerStream::new(key);
        assert!(matches!(
            KdbxXmlWriter::write(&db, &mut write_stream),
            Err(crate::DatabaseError::Unsupported(_))
        ));
    }

    #[test]
    fn test_xml_nesting_limit_rejected() {
        let depth = crate::kdbx::limits::MAX_XML_NESTING_DEPTH + 1;
        let xml = format!("{}{}", "<x>".repeat(depth), "</x>".repeat(depth));
        let mut stream = Salsa20InnerStream::new(b"nesting-limit");
        assert!(matches!(
            KdbxXmlReader::read(&xml, &mut stream),
            Err(crate::DatabaseError::InvalidFormat(_))
        ));
    }

    #[test]
    fn test_xml_empty_group_roundtrip() {
        let mut db = Database {
            name: "EmptyGroupDB".to_string(),
            ..Database::default()
        };

        let root_id =
            NodeId::from_uuid(Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap());
        let mut root = Group::new(root_id);
        root.title = "Root".to_string();

        let empty_id =
            NodeId::from_uuid(Uuid::parse_str("11111111-aaaa-2222-bbbb-333333333333").unwrap());
        let mut empty_group = Group::new(empty_id);
        empty_group.title = "EmptySub".to_string();

        root.add_child_group(empty_id);
        db.groups.insert(empty_id, empty_group);
        db.groups.insert(root_id, root);
        db.root_group_id = Some(root_id);

        let key = b"empty_grp_key_123";
        let mut is_w = Salsa20InnerStream::new(key);
        let xml = KdbxXmlWriter::write(&db, &mut is_w).unwrap();

        let mut is_r = Salsa20InnerStream::new(key);
        let db2 = KdbxXmlReader::read(&xml, &mut is_r).unwrap();

        assert_eq!(db2.name, "EmptyGroupDB");
        // 重构后 root_group 也存放在 db.groups 中（单一真源），所以总数 = root + 1 子组 = 2。
        assert_eq!(
            db2.groups.len(),
            2,
            "should have root + exactly one subgroup"
        );
        let root = db2.root_group().unwrap();
        assert_eq!(root.child_group_ids.len(), 1);
        let empty = db2.groups.get(&root.child_group_ids[0]).unwrap();
        assert_eq!(empty.title, "EmptySub");
        assert!(empty.child_entry_ids.is_empty());
        assert!(empty.child_group_ids.is_empty());
    }

    #[test]
    fn test_xml_special_characters() {
        let mut db = Database {
            name: "SpecialCharsDB".to_string(),
            ..Database::default()
        };

        let root_id =
            NodeId::from_uuid(Uuid::parse_str("cccccccc-dddd-eeee-1111-222222222222").unwrap());
        let mut root = Group::new(root_id);
        root.title = "Root".to_string();

        let entry_id =
            NodeId::from_uuid(Uuid::parse_str("99999999-8888-7777-6666-555555555555").unwrap());
        let mut entry = Entry::new(entry_id);
        entry.title = r#"Title with <>&"special"#.to_string();
        entry.username = ProtectedString::new_plain("user");
        entry.password = ProtectedString::new_protected("pass");

        root.add_child_entry(entry_id);
        db.entries.insert(entry_id, entry);
        db.groups.insert(root_id, root);
        db.root_group_id = Some(root_id);

        let key = b"special_chars_key";
        let mut is_w = Salsa20InnerStream::new(key);
        let xml = KdbxXmlWriter::write(&db, &mut is_w).unwrap();

        assert!(!xml.contains(r#"Title with <>&"special"#));

        let mut is_r = Salsa20InnerStream::new(key);
        let db2 = KdbxXmlReader::read(&xml, &mut is_r).unwrap();

        let entry2 = db2.entries.values().next().unwrap();
        assert_eq!(entry2.title, r#"Title with <>&"special"#);
    }

    #[test]
    fn test_xml_multiple_groups() {
        let mut db = Database {
            name: "NestedDB".to_string(),
            ..Database::default()
        };

        let root_id =
            NodeId::from_uuid(Uuid::parse_str("aa000000-0000-0000-0000-000000000001").unwrap());
        let mut root = Group::new(root_id);
        root.title = "Root".to_string();

        let g1_id =
            NodeId::from_uuid(Uuid::parse_str("aa000000-0000-0000-0000-000000000002").unwrap());
        let mut g1 = Group::new(g1_id);
        g1.title = "Level1".to_string();

        let g2_id =
            NodeId::from_uuid(Uuid::parse_str("aa000000-0000-0000-0000-000000000003").unwrap());
        let mut g2 = Group::new(g2_id);
        g2.title = "Level2".to_string();

        let g3_id =
            NodeId::from_uuid(Uuid::parse_str("aa000000-0000-0000-0000-000000000004").unwrap());
        let mut g3 = Group::new(g3_id);
        g3.title = "Level3".to_string();

        g2.add_child_group(g3_id);
        g1.add_child_group(g2_id);
        root.add_child_group(g1_id);

        db.groups.insert(g1_id, g1);
        db.groups.insert(g2_id, g2);
        db.groups.insert(g3_id, g3);
        db.groups.insert(root_id, root);
        db.root_group_id = Some(root_id);

        let key = b"nested_groups_key";
        let mut is_w = Salsa20InnerStream::new(key);
        let xml = KdbxXmlWriter::write(&db, &mut is_w).unwrap();

        let mut is_r = Salsa20InnerStream::new(key);
        let db2 = KdbxXmlReader::read(&xml, &mut is_r).unwrap();

        assert_eq!(db2.name, "NestedDB");
        // 重构后 root_group 也存放在 db.groups 中（单一真源），所以总数 = root + 3 子组 = 4。
        assert_eq!(db2.groups.len(), 4);

        let titles: std::collections::HashSet<String> =
            db2.groups.values().map(|g| g.title.clone()).collect();
        assert!(titles.contains("Level1"));
        assert!(titles.contains("Level2"));
        assert!(titles.contains("Level3"));

        let root = db2.root_group().unwrap();
        assert_eq!(root.child_group_ids.len(), 1);
        let g1 = db2.groups.get(&root.child_group_ids[0]).unwrap();
        assert_eq!(g1.title, "Level1");
        assert_eq!(g1.child_group_ids.len(), 1);
        let g2 = db2.groups.get(&g1.child_group_ids[0]).unwrap();
        assert_eq!(g2.title, "Level2");
        assert_eq!(g2.child_group_ids.len(), 1);
        let g3 = db2.groups.get(&g2.child_group_ids[0]).unwrap();
        assert_eq!(g3.title, "Level3");
        assert!(g3.child_group_ids.is_empty());
    }
}
