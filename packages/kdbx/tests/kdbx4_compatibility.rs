use std::fs::File;
use std::io::Cursor;

use keeless_kdbx::{
    open_database, save_database, CompositeKey, Database, DatabaseVersion, Entry, EntryBinary,
    Group, NodeId, ProtectedString,
};

#[test]
fn opens_keepass_rs_kdbx41_aes_fixture() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/resources/test_db_kdbx4_with_password_aes.kdbx"
    );
    let database = open_database(
        File::open(path).expect("fixture should exist"),
        &CompositeKey::new().with_password(b"demopass").unwrap(),
    )
    .expect("external KDBX4.1 fixture should open");

    assert_eq!(database.version, DatabaseVersion::KDBX4);
    assert_eq!(database.file_version, 0x0004_0001);
    assert_eq!(database.entry_count(), 1);
    assert!(
        database
            .root_group()
            .expect("fixture should have a root group")
            .creation_time
            .as_millis()
            .is_some(),
        "KDBX4 Base64 timestamps should be decoded"
    );
}

#[test]
fn opens_external_argon2id_aes_fixture() {
    assert_external_fixture_opens("test_db_kdbx4_with_password_argon2id.kdbx", 2);
}

#[test]
fn opens_external_argon2id_chacha20_fixture() {
    assert_external_fixture_opens("test_db_kdbx4_with_password_argon2id_chacha20.kdbx", 1);
}

fn assert_external_fixture_opens(name: &str, expected_entries: usize) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/resources")
        .join(name);
    let database = open_database(
        File::open(path).expect("fixture should exist"),
        &CompositeKey::new().with_password(b"demopass").unwrap(),
    )
    .expect("external KDBX4 fixture should open");

    assert_eq!(database.version, DatabaseVersion::KDBX4);
    assert_eq!(database.entry_count(), expected_entries);
}

#[test]
fn output_opens_with_independent_keepass_parser() {
    let mut database = Database::new(DatabaseVersion::KDBX4);
    let root_id = NodeId::new_uuid();
    let mut root = Group::new(root_id);
    root.title = "Root".into();

    let entry_id = NodeId::new_uuid();
    let mut entry = Entry::new(entry_id);
    entry.title = "Interoperability".into();
    entry.password = ProtectedString::new_protected("secret");
    entry.binaries.push(EntryBinary {
        name: "protected.bin".into(),
        data: vec![0, 1, 2, 255],
        is_protected: true,
    });
    root.add_child_entry(entry_id);
    database.groups.insert(root_id, root);
    database.entries.insert(entry_id, entry);
    database.root_group_id = Some(root_id);

    let password = "demopass";
    let mut encoded = Vec::new();
    save_database(
        &mut encoded,
        &database,
        &CompositeKey::new()
            .with_password(password.as_bytes())
            .unwrap(),
    )
    .expect("database should save");

    let opened = keepass::Database::open(
        &mut Cursor::new(encoded),
        keepass::DatabaseKey::new().with_password(password),
    )
    .expect("independent parser should open generated KDBX4");

    assert_eq!(opened.root().name, "Root");
    assert_eq!(opened.root().entries().count(), 1);
    assert_eq!(
        opened
            .root()
            .entries()
            .next()
            .expect("generated entry should exist")
            .attachments()
            .count(),
        1
    );
}
