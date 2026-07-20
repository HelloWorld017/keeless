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
    use crate::crypto::inner_stream::{InnerStreamCipher, Salsa20InnerStream};
    use crate::kdbx::limits::MAX_XML_NESTING_DEPTH;
    use crate::model::core::node::NodeId;
    use crate::model::core::security::ProtectedString;
    use crate::model::db::database::{Database, DatabaseVersion, EntryFieldUpdate};
    use crate::model::entry::Entry;
    use crate::model::exception::DatabaseResult;
    use crate::model::group::Group;
    use crate::model::DeletedObject;
    use crate::DatabaseError;
    use base64::Engine;
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
        entry.title = "Test Entry".into();
        entry.username = ProtectedString::new_plain("user@test.com");
        entry.password = ProtectedString::new_protected("s3cret!");
        entry.url = "https://example.com".into();

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
        let mut is_write = Salsa20InnerStream::new(key).unwrap();
        let xml = KdbxXmlWriter::write(&db, &mut is_write).unwrap();
        assert!(xml.contains("<KeePassFile>"));
        assert!(xml.contains("<DatabaseName>TestDB</DatabaseName>"));

        // Read
        let mut is_read = Salsa20InnerStream::new(key).unwrap();
        let db2 = KdbxXmlReader::read(&xml, &mut is_read).unwrap();
        assert_eq!(db2.name, "TestDB");
        assert_eq!(db2.entries.len(), 1);
        assert_eq!(db2.entries.len(), 1);
    }

    #[test]
    fn test_protected_field_roundtrip() {
        let db = make_test_db();
        let key = b"key_for_protected";

        let mut is_w = Salsa20InnerStream::new(key).unwrap();
        let xml = KdbxXmlWriter::write(&db, &mut is_w).unwrap();

        // Verify Password is base64-encoded (not plaintext)
        assert!(!xml.contains("s3cret!"));
        assert!(xml.contains("Protected=\"True\""));
        assert!(!xml.contains("ProtectInMemory"));

        // Read back
        let mut is_r = Salsa20InnerStream::new(key).unwrap();
        let db2 = KdbxXmlReader::read(&xml, &mut is_r).unwrap();

        let entry = db2.entries.values().next().unwrap();
        assert_eq!(entry.password.as_str(), "s3cret!");
        assert!(entry.password.is_protected());
        assert!(!entry.username.is_protected());
    }

    #[test]
    fn test_invalid_protected_value_is_format_error() {
        let xml = r#"<KeePassFile><Root><Group><UUID>obLD1OX2eJCrze8SNFZ4kA</UUID><Name>Root</Name><Entry><UUID>ERERESIiMzNERFVVVVVVVQ</UUID><String><Key>Password</Key><Value Protected="True">not-base64!</Value></String></Entry></Group></Root></KeePassFile>"#;
        let mut stream = Salsa20InnerStream::new(b"invalid-value").unwrap();
        assert!(matches!(
            KdbxXmlReader::read(xml, &mut stream),
            Err(DatabaseError::InvalidFormat(_))
        ));
    }

    #[test]
    fn test_kdbx31_meta_binary_reference_is_resolved_by_id() {
        let pool = (0..=5)
            .map(|id| {
                let data =
                    base64::engine::general_purpose::STANDARD.encode(format!("attachment-{id}"));
                format!(r#"<Binary ID="{id}" Compressed="False">{data}</Binary>"#)
            })
            .collect::<String>();
        let xml = format!(
            r#"<KeePassFile><Meta><Binaries>{pool}</Binaries></Meta><Root><Group><UUID>obLD1OX2eJCrze8SNFZ4kA</UUID><Entry><UUID>ERERESIiMzNERFVVVVVVVQ</UUID><Binary><Key>file.txt</Key><Value Ref="4"/></Binary></Entry></Group></Root></KeePassFile>"#
        );
        let mut stream = Salsa20InnerStream::new(b"kdbx31-binary-ref").unwrap();

        let db = KdbxXmlReader::read(&xml, &mut stream).unwrap();
        let binary = &db.entries.values().next().unwrap().binaries[0];
        assert_eq!(binary.name, "file.txt");
        assert_eq!(binary.data, b"attachment-4");
        assert!(!binary.is_protected);
    }

    #[test]
    fn test_kdbx31_compressed_meta_binary_is_decompressed() {
        let expected = b"compressed attachment";
        let compressed = crate::crypto::compression::compress(expected).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(compressed);
        let xml = format!(
            r#"<KeePassFile><Meta><Binaries><Binary ID="7" Compressed="True">{encoded}</Binary></Binaries></Meta><Root><Group><UUID>obLD1OX2eJCrze8SNFZ4kA</UUID><Entry><UUID>ERERESIiMzNERFVVVVVVVQ</UUID><Binary><Key>file.bin</Key><Value Ref="7"/></Binary></Entry></Group></Root></KeePassFile>"#
        );
        let mut stream = Salsa20InnerStream::new(b"compressed-binary").unwrap();

        let db = KdbxXmlReader::read(&xml, &mut stream).unwrap();
        assert_eq!(
            db.entries.values().next().unwrap().binaries[0].data,
            expected
        );
    }

    #[test]
    fn test_kdbx4_binary_reference_remains_positional() {
        let xml = r#"<KeePassFile><Meta></Meta><Root><Group><UUID>obLD1OX2eJCrze8SNFZ4kA</UUID><Entry><UUID>ERERESIiMzNERFVVVVVVVQ</UUID><Binary><Key>file.bin</Key><Value Ref="1"/></Binary></Entry></Group></Root></KeePassFile>"#;
        let binaries = vec![(b"zero".to_vec(), false), (b"one".to_vec(), true)];
        let mut stream = Salsa20InnerStream::new(b"kdbx4-binary-ref").unwrap();

        let db = KdbxXmlReader::read_with_binaries(xml, &mut stream, &binaries).unwrap();
        let binary = &db.entries.values().next().unwrap().binaries[0];
        assert_eq!(binary.data, b"one");
        assert!(binary.is_protected);
    }

    #[test]
    fn test_deleted_object_time_roundtrip() {
        let mut db = make_test_db();
        let deleted_id = NodeId::new_uuid();
        db.deleted_objects.push(DeletedObject {
            id: deleted_id,
            deletion_time: 1_725_000_123_000,
        });
        let key = b"deleted-time";
        let mut write_stream = Salsa20InnerStream::new(key).unwrap();
        let xml = KdbxXmlWriter::write(&db, &mut write_stream).unwrap();
        let mut read_stream = Salsa20InnerStream::new(key).unwrap();
        let loaded = KdbxXmlReader::read(&xml, &mut read_stream).unwrap();
        assert_eq!(loaded.deleted_objects[0].id, deleted_id);
        assert_eq!(loaded.deleted_objects[0].deletion_time, 1_725_000_123_000);
    }

    #[test]
    fn test_unknown_xml_is_preserved_when_saving() {
        let xml = r#"<KeePassFile><Meta><FutureMeta mode="new"><Nested>meta</Nested></FutureMeta></Meta><Root><Group><UUID>obLD1OX2eJCrze8SNFZ4kA</UUID><Name>Root</Name><FutureElement kind="test"><Value>x</Value><Empty flag="1"/></FutureElement></Group></Root><FutureFile/></KeePassFile>"#;
        let key = b"unsupported-xml";
        let mut read_stream = Salsa20InnerStream::new(key).unwrap();
        let mut db = KdbxXmlReader::read(xml, &mut read_stream).unwrap();
        assert!(db.contains_unsupported_xml);
        db.name = "Changed".to_string();

        let mut write_stream = Salsa20InnerStream::new(key).unwrap();
        let output = KdbxXmlWriter::write(&db, &mut write_stream).unwrap();
        assert!(output.contains(r#"<FutureMeta mode="new"><Nested>meta</Nested></FutureMeta>"#));
        assert!(output.contains(
            r#"<FutureElement kind="test"><Value>x</Value><Empty flag="1"/></FutureElement>"#
        ));
        assert!(output.contains("<FutureFile/>"));
        assert!(output.contains("<DatabaseName>Changed</DatabaseName>"));

        let mut reread_stream = Salsa20InnerStream::new(key).unwrap();
        let reread = KdbxXmlReader::read(&output, &mut reread_stream).unwrap();
        assert!(reread.contains_unsupported_xml);
        assert_eq!(reread.name, "Changed");
    }

    #[test]
    fn duplicate_custom_string_extensions_follow_fields_through_updates() {
        let xml = r#"<KeePassFile><Meta></Meta><Root><Group><UUID>obLD1OX2eJCrze8SNFZ4kA</UUID><Entry><UUID>ERERESIiMzNERFVVVVVVVQ</UUID><String><Key>Title</Key><Value>title</Value></String><String><Key>UserName</Key><Value>user</Value></String><String><Key>Password</Key><Value>password</Value></String><String><Key>URL</Key><Value>url</Value></String><String><Key>Notes</Key><Value>notes</Value></String><String><Key>Duplicate</Key><Value>first</Value><First/></String><String><Key>Duplicate</Key><Value>second</Value><Second/></String><String><Key>Delete</Key><Value>delete</Value><Deleted/></String></Entry></Group></Root></KeePassFile>"#;
        let stream_key = b"custom-extensions";
        let mut read_stream = Salsa20InnerStream::new(stream_key).unwrap();
        let mut db = KdbxXmlReader::read(xml, &mut read_stream).unwrap();
        let entry_id = *db.entries.keys().next().unwrap();
        let fields = [
            (Some(0), "Title", "title"),
            (Some(1), "UserName", "user"),
            (Some(2), "Password", "password"),
            (Some(3), "URL", "url"),
            (Some(4), "Notes", "notes"),
            (Some(6), "Renamed", "second"),
            (Some(5), "Duplicate", "first"),
            (None, "Added", "added"),
        ]
        .into_iter()
        .map(|(field_index, name, value)| EntryFieldUpdate {
            field_index,
            name: name.into(),
            value: Some(value.into()),
            is_protected: false,
        })
        .collect::<Vec<_>>();
        let key = crate::CompositeKey::new().with_password(b"test").unwrap();

        assert!(db.update_entry_fields(&key, &entry_id, &fields).unwrap());
        db.entries.get_mut(&entry_id).unwrap().history.clear();
        let mut write_stream = Salsa20InnerStream::new(stream_key).unwrap();
        let output = KdbxXmlWriter::write(&db, &mut write_stream).unwrap();
        assert!(
            output.contains("<String><Key>Renamed</Key><Value>second</Value><Second/></String>")
        );
        assert!(
            output.contains("<String><Key>Duplicate</Key><Value>first</Value><First/></String>")
        );
        assert!(output.contains("<String><Key>Added</Key><Value>added</Value></String>"));
        assert!(!output.contains("<Deleted/>"));

        let mut reread_stream = Salsa20InnerStream::new(stream_key).unwrap();
        let reread = KdbxXmlReader::read(&output, &mut reread_stream).unwrap();
        assert_eq!(
            reread.entries[&entry_id]
                .custom_fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["Renamed", "Duplicate", "Added"]
        );
    }

    #[test]
    fn test_protected_value_inside_unknown_xml_keeps_stream_aligned() -> DatabaseResult<()> {
        let key = b"protected-extension";
        let mut encrypt_stream = Salsa20InnerStream::new(key).unwrap();
        let mut extension_value = b"future-secret".to_vec();
        encrypt_stream.process(&mut extension_value)?;
        let mut password = b"password".to_vec();
        encrypt_stream.process(&mut password)?;
        let extension_value = base64::engine::general_purpose::STANDARD.encode(extension_value);
        let password = base64::engine::general_purpose::STANDARD.encode(password);
        let xml = format!(
            r#"<KeePassFile><Meta><Future Protected="True">{extension_value}</Future></Meta><Root><Group><UUID>obLD1OX2eJCrze8SNFZ4kA</UUID><Entry><UUID>ERERESIiMzNERFVVVVVVVQ</UUID><String><Key>Password</Key><Value Protected="True">{password}</Value></String></Entry></Group></Root></KeePassFile>"#
        );

        let mut read_stream = Salsa20InnerStream::new(key).unwrap();
        let db = KdbxXmlReader::read(&xml, &mut read_stream).unwrap();
        assert_eq!(
            db.entries.values().next().unwrap().password.as_str(),
            "password"
        );

        let mut write_stream = Salsa20InnerStream::new(key).unwrap();
        let output = KdbxXmlWriter::write(&db, &mut write_stream).unwrap();
        assert!(!output.contains("future-secret"));
        let mut reread_stream = Salsa20InnerStream::new(key).unwrap();
        let reread = KdbxXmlReader::read(&output, &mut reread_stream).unwrap();
        assert_eq!(
            reread.entries.values().next().unwrap().password.as_str(),
            "password"
        );
        Ok(())
    }

    #[test]
    fn test_missing_required_xml_values_are_rejected() {
        let cases = [
            r#"<KeePassFile><Meta/><Root><Group><Name>Root</Name></Group></Root></KeePassFile>"#,
            r#"<KeePassFile><Meta/><Root><Group><UUID>obLD1OX2eJCrze8SNFZ4kA</UUID><Entry><String><Key>Title</Key><Value>x</Value></String></Entry></Group></Root></KeePassFile>"#,
            r#"<KeePassFile><Meta/><Root><Group><UUID>obLD1OX2eJCrze8SNFZ4kA</UUID><Entry><UUID>ERERESIiMzNERFVVVVVVVQ</UUID><String><Value>x</Value></String></Entry></Group></Root></KeePassFile>"#,
            r#"<KeePassFile><Meta/><Root><Group><UUID>obLD1OX2eJCrze8SNFZ4kA</UUID></Group><DeletedObjects><DeletedObject><UUID>ERERESIiMzNERFVVVVVVVQ</UUID></DeletedObject></DeletedObjects></Root></KeePassFile>"#,
        ];

        for xml in cases {
            let mut stream = Salsa20InnerStream::new(b"required-values").unwrap();
            assert!(matches!(
                KdbxXmlReader::read(xml, &mut stream),
                Err(DatabaseError::InvalidFormat(_))
            ));
        }
    }

    #[test]
    fn test_unknown_xml_is_preserved_in_nested_kdbx_containers() {
        let xml = r#"<KeePassFile><Meta><MemoryProtection><FutureMemory>m</FutureMemory></MemoryProtection><CustomIcons><Icon><UUID>ERERESIiMzNERFVVVVVVVQ</UUID><Data></Data><FutureIcon>i</FutureIcon></Icon><FutureIcons/></CustomIcons><CustomData><Item><Key>meta</Key><Value>value</Value><FutureItem>mi</FutureItem></Item><FutureMetaData/></CustomData></Meta><Root><Group><UUID>obLD1OX2eJCrze8SNFZ4kA</UUID><Times><FutureGroupTime>gt</FutureGroupTime></Times><CustomData><Item><Key>group</Key><Value>value</Value><FutureGroupItem/></Item></CustomData><Entry><UUID>mZmZiYiId3dmZlVVVVVVVQ</UUID><Times><FutureEntryTime>et</FutureEntryTime></Times><String><Key>Title</Key><Value>title</Value><FutureString>s</FutureString></String><Binary><Key>file</Key><Value></Value><FutureBinary>b</FutureBinary></Binary><AutoType><Association><Window></Window><KeystrokeSequence></KeystrokeSequence><FutureAssociation>a</FutureAssociation></Association><FutureAutoType/></AutoType><CustomData><Item><Key>entry</Key><Value>value</Value><FutureEntryItem/></Item></CustomData><History><FutureHistory/></History><FutureEntry/></Entry><FutureGroup/></Group><DeletedObjects><DeletedObject><UUID>qqqqqru7zMzd3e7u7u7u7g</UUID><DeletionTime>2024-01-01T00:00:00Z</DeletionTime><FutureDeleted>d</FutureDeleted></DeletedObject><FutureDeletedObjects/></DeletedObjects><FutureRoot/></Root></KeePassFile>"#;
        let key = b"nested-extensions";
        let mut read_stream = Salsa20InnerStream::new(key).unwrap();
        let mut db = KdbxXmlReader::read(xml, &mut read_stream).unwrap();
        db.version = DatabaseVersion::KDBX31;
        let mut write_stream = Salsa20InnerStream::new(key).unwrap();
        let output = KdbxXmlWriter::write(&db, &mut write_stream).unwrap();

        for name in [
            "FutureMemory",
            "FutureIcon",
            "FutureIcons",
            "FutureItem",
            "FutureMetaData",
            "FutureGroupTime",
            "FutureGroupItem",
            "FutureEntryTime",
            "FutureString",
            "FutureBinary",
            "FutureAssociation",
            "FutureAutoType",
            "FutureEntryItem",
            "FutureHistory",
            "FutureEntry",
            "FutureGroup",
            "FutureDeleted",
            "FutureDeletedObjects",
            "FutureRoot",
        ] {
            assert!(output.contains(&format!("<{name}")), "missing {name}");
        }

        let mut reread_stream = Salsa20InnerStream::new(key).unwrap();
        KdbxXmlReader::read(&output, &mut reread_stream).unwrap();
    }

    #[test]
    fn test_xml_nesting_limit_rejected() {
        let depth = MAX_XML_NESTING_DEPTH + 1;
        let xml = format!("{}{}", "<x>".repeat(depth), "</x>".repeat(depth));
        let mut stream = Salsa20InnerStream::new(b"nesting-limit").unwrap();
        assert!(matches!(
            KdbxXmlReader::read(&xml, &mut stream),
            Err(DatabaseError::InvalidFormat(_))
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
        let mut is_w = Salsa20InnerStream::new(key).unwrap();
        let xml = KdbxXmlWriter::write(&db, &mut is_w).unwrap();

        let mut is_r = Salsa20InnerStream::new(key).unwrap();
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
        entry.title = r#"Title with <>&"special"#.into();
        entry.username = ProtectedString::new_plain("user");
        entry.password = ProtectedString::new_protected("pass");

        root.add_child_entry(entry_id);
        db.entries.insert(entry_id, entry);
        db.groups.insert(root_id, root);
        db.root_group_id = Some(root_id);

        let key = b"special_chars_key";
        let mut is_w = Salsa20InnerStream::new(key).unwrap();
        let xml = KdbxXmlWriter::write(&db, &mut is_w).unwrap();

        assert!(!xml.contains(r#"Title with <>&"special"#));

        let mut is_r = Salsa20InnerStream::new(key).unwrap();
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
        let mut is_w = Salsa20InnerStream::new(key).unwrap();
        let xml = KdbxXmlWriter::write(&db, &mut is_w).unwrap();

        let mut is_r = Salsa20InnerStream::new(key).unwrap();
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
