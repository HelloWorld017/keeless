use std::path::Path;

use keeless_kdbx::crypto::compression::CompressionAlgorithm;
use keeless_kdbx::{
    open_database, save_database, CompositeKey, Database, DatabaseError, DatabaseVersion,
    EntryFieldSelector,
};
use sha2::{Digest, Sha256};

const KDBXWEB_EMPTY_PASS_SHA256: &str =
    "4af2c576a50caa7d3104817453abe7843416e362548c2af3e2721976a434d49e";
const KDBXWEB_KDBX41_SHA256: &str =
    "189535d9f2c097756f1b93f9985727643097d56ce07243a871f27a03268f2906";
const KDBXWEB_CYRILLIC_SHA256: &str =
    "552501ed6c218c37d58198750e924db6ab6ee1858076e7851228387b44af70d5";
const KEEPASSXC_BROKEN_HEADER_SHA256: &str =
    "681e9117b297ee7be9d569f066b07f2a361d1bf0aae49074033022425f267a00";
const KEEPASSXC_COMPRESSED_SHA256: &str =
    "392b089bf3f17f7e507dc2d97584493f1b709f6989b37c2b964bfc8b21e994b8";
const KEEPASSXC_FORMAT400_SHA256: &str =
    "4f23b3f5f6c71209e135dab92d0f06034315d3f70e3e2ef3ec3a77ff37b49435";
const KEEPASSXC_NON_ASCII_SHA256: &str =
    "e8a69ba7e0cbd86a98b9a9d8fb6df8deecb052a4a3da842b200776434cb760e4";
const KEEPASSXC_PROTECTED_STRINGS_SHA256: &str =
    "15efb80917ffff30173e75d5bb53c270a10076b26a90f4a80d2071b71f318d8c";

fn fixture_bytes(source: &str, name: &str, expected_sha256: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/resources/upstream")
        .join(source)
        .join(name);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|error| panic!("failed to read fixture {}: {error}", path.display()));
    let actual_sha256 = hex::encode(Sha256::digest(&bytes));
    assert_eq!(
        actual_sha256,
        expected_sha256,
        "fixture checksum changed: {}",
        path.display()
    );
    bytes
}

fn key(password: &str) -> CompositeKey {
    CompositeKey::new()
        .with_password(password.as_bytes())
        .unwrap()
}

fn open_fixture(source: &str, name: &str, expected_sha256: &str, password: &str) -> Database {
    let bytes = fixture_bytes(source, name, expected_sha256);
    open_database(bytes.as_slice(), &key(password))
        .unwrap_or_else(|error| panic!("failed to open {source}/{name}: {error}"))
}

fn round_trip(database: &Database, password: &str) -> Database {
    let key = key(password);
    let mut encoded = Vec::new();
    save_database(&mut encoded, database, &key).expect("external database should save");
    open_database(encoded.as_slice(), &key).expect("saved external database should reopen")
}

#[test]
fn opens_and_preserves_kdbxweb_kdbx41_extensions() {
    let database = open_fixture("kdbxweb", "KDBX4.1.kdbx", KDBXWEB_KDBX41_SHA256, "test");

    assert_eq!(database.version, DatabaseVersion::KDBX4);
    assert_eq!(database.file_version, 0x0004_0001);
    assert_eq!(database.custom_icons.len(), 2);
    assert!(database
        .groups
        .values()
        .any(|group| group.title == "With tags"));
    assert!(database
        .entries
        .values()
        .any(|entry| entry.title().as_str() == "DisabledQ"));
    assert!(database
        .entries
        .values()
        .any(|entry| entry.title().as_str() == "Was inside"));
    assert!(database.contains_unsupported_xml);

    let reopened = round_trip(&database, "test");
    assert_eq!(reopened.file_version, 0x0004_0001);
    assert_eq!(reopened.custom_icons.len(), 2);
    assert!(reopened
        .groups
        .values()
        .any(|group| group.title == "With tags"));
    assert!(reopened
        .entries
        .values()
        .any(|entry| entry.title().as_str() == "DisabledQ"));
    assert!(reopened
        .entries
        .values()
        .any(|entry| entry.title().as_str() == "Was inside"));
    assert!(reopened.contains_unsupported_xml);
}

#[test]
fn opens_and_preserves_kdbxweb_cyrillic_fixture() {
    let password = "пароль";
    let database = open_fixture(
        "kdbxweb",
        "cyrillic.kdbx",
        KDBXWEB_CYRILLIC_SHA256,
        password,
    );

    assert_eq!(database.version, DatabaseVersion::KDBX31);
    assert_eq!(database.compression, CompressionAlgorithm::None);
    assert_eq!(database.name, "моя база паролей");
    assert_eq!(database.default_username, "пользователь");
    assert_eq!(database.group_count(), 7);
    assert_eq!(database.entry_count(), 2);

    let entry = database
        .entries
        .values()
        .find(|entry| entry.title().as_str() == "моя запись")
        .expect("Cyrillic entry should exist");
    assert_eq!(entry.tags, ["теги"]);
    assert_eq!(entry.history.len(), 1);
    assert!(entry
        .custom_fields()
        .any(|(_, field)| field.name() == "поле1" && field.value().as_str() == "значение1"));
    assert_eq!(
        database
            .with_entry_field(
                &key(password),
                &entry.id,
                &EntryFieldSelector::Password,
                str::to_owned,
            )
            .unwrap(),
        "пароль"
    );
    assert_eq!(
        database
            .with_entry_field(
                &key(password),
                &entry.id,
                &EntryFieldSelector::Custom("поле2".into()),
                str::to_owned,
            )
            .unwrap(),
        "значение2"
    );

    let reopened = round_trip(&database, password);
    assert_eq!(reopened.name, "моя база паролей");
    assert_eq!(reopened.group_count(), 7);
    assert_eq!(reopened.entry_count(), 2);
    assert!(reopened.entries.values().any(|entry| {
        entry.title().as_str() == "моя запись" && entry.tags == ["теги"] && entry.history.len() == 1
    }));

    let mut encoded = Vec::new();
    save_database(&mut encoded, &database, &key(password)).unwrap();
    let independent = keepass::Database::open(
        &mut encoded.as_slice(),
        keepass::DatabaseKey::new().with_password(password),
    )
    .expect("independent parser should open generated KDBX 3.1 output");
    assert_eq!(independent.root().name, "cyrillic");
}

#[test]
fn distinguishes_an_explicit_empty_password() {
    let bytes = fixture_bytes("kdbxweb", "EmptyPass.kdbx", KDBXWEB_EMPTY_PASS_SHA256);
    let database = open_database(bytes.as_slice(), &key("")).expect("empty password should open");
    assert_eq!(database.version, DatabaseVersion::KDBX31);

    assert!(matches!(
        open_database(bytes.as_slice(), &CompositeKey::new()),
        Err(DatabaseError::InvalidKey)
    ));
    assert!(open_database(bytes.as_slice(), &key("not-empty")).is_err());

    let reopened = round_trip(&database, "");
    assert_eq!(reopened.group_count(), database.group_count());
    assert_eq!(reopened.entry_count(), database.entry_count());
}

#[test]
fn opens_and_preserves_keepassxc_format400_content() {
    let database = open_fixture(
        "keepassxc",
        "Format400.kdbx",
        KEEPASSXC_FORMAT400_SHA256,
        "t",
    );

    assert_eq!(database.version, DatabaseVersion::KDBX4);
    assert_eq!(database.file_version, 0x0004_0000);
    assert_eq!(database.name, "Format400");
    assert_eq!(database.root_group().unwrap().title, "Format400");
    assert_eq!(database.entry_count(), 1);
    let entry = database.entries.values().next().unwrap();
    assert_eq!(entry.title().as_str(), "Format400");
    assert!(entry
        .custom_fields()
        .any(|(_, field)| field.name() == "Format400"));
    assert_eq!(
        database
            .with_entry_field(
                &key("t"),
                &entry.id,
                &EntryFieldSelector::Custom("Format400".into()),
                str::to_owned,
            )
            .unwrap(),
        "Format400"
    );
    assert_eq!(entry.binaries.len(), 1);
    assert_eq!(entry.binaries[0].name, "Format400");
    assert_eq!(entry.binaries[0].data, b"Format400\n");

    let reopened = round_trip(&database, "t");
    let reopened_entry = reopened.entries.values().next().unwrap();
    assert_eq!(reopened.name, "Format400");
    assert_eq!(reopened_entry.binaries[0].data, b"Format400\n");

    let bytes = fixture_bytes("keepassxc", "Format400.kdbx", KEEPASSXC_FORMAT400_SHA256);
    assert!(matches!(
        open_database(bytes.as_slice(), &key("wrong")),
        Err(DatabaseError::InvalidCredentials)
    ));
}

#[test]
fn opens_keepassxc_compression_and_unicode_password_fixtures() {
    let compressed = open_fixture(
        "keepassxc",
        "Compressed.kdbx",
        KEEPASSXC_COMPRESSED_SHA256,
        "",
    );
    assert_eq!(compressed.version, DatabaseVersion::KDBX31);
    assert_eq!(compressed.name, "Compressed");
    assert_eq!(compressed.compression, CompressionAlgorithm::Gzip);

    let non_ascii = open_fixture(
        "keepassxc",
        "NonAscii.kdbx",
        KEEPASSXC_NON_ASCII_SHA256,
        "Δöض",
    );
    assert_eq!(non_ascii.version, DatabaseVersion::KDBX31);
    assert_eq!(non_ascii.name, "NonAsciiTest");
    assert_eq!(non_ascii.compression, CompressionAlgorithm::None);

    let compressed_reopened = round_trip(&compressed, "");
    let non_ascii_reopened = round_trip(&non_ascii, "Δöض");
    assert_eq!(compressed_reopened.compression, CompressionAlgorithm::Gzip);
    assert_eq!(non_ascii_reopened.name, "NonAsciiTest");
}

#[test]
fn opens_and_preserves_keepassxc_protected_strings() {
    let password = "masterpw";
    let database = open_fixture(
        "keepassxc",
        "ProtectedStrings.kdbx",
        KEEPASSXC_PROTECTED_STRINGS_SHA256,
        password,
    );
    assert_eq!(database.name, "Protected Strings Test");

    let entry = database.entries.values().next().unwrap();
    assert_eq!(entry.title().as_str(), "Sample Entry");
    assert!(entry.password().is_memory_protected());
    assert_eq!(
        database
            .with_entry_field(
                &key(password),
                &entry.id,
                &EntryFieldSelector::UserName,
                str::to_owned,
            )
            .unwrap(),
        "Protected User Name"
    );
    assert_eq!(
        database
            .with_entry_field(
                &key(password),
                &entry.id,
                &EntryFieldSelector::Password,
                str::to_owned,
            )
            .unwrap(),
        "ProtectedPassword"
    );
    assert!(entry.custom_fields().any(|(_, field)| {
        field.name() == "TestProtected" && field.value().is_memory_protected()
    }));
    assert!(entry.custom_fields().any(|(_, field)| {
        field.name() == "TestUnprotected"
            && !field.value().is_protected()
            && field.value().as_str() == "DEF"
    }));
    assert_eq!(
        database
            .with_entry_field(
                &key(password),
                &entry.id,
                &EntryFieldSelector::Custom("TestProtected".into()),
                str::to_owned,
            )
            .unwrap(),
        "ABC"
    );

    let reopened = round_trip(&database, password);
    let reopened_entry = reopened.entries.values().next().unwrap();
    assert_eq!(
        reopened
            .with_entry_field(
                &key(password),
                &reopened_entry.id,
                &EntryFieldSelector::Custom("TestProtected".into()),
                str::to_owned,
            )
            .unwrap(),
        "ABC"
    );
}

#[test]
fn rejects_keepassxc_fixture_with_broken_header_hash() {
    let bytes = fixture_bytes(
        "keepassxc",
        "BrokenHeaderHash.kdbx",
        KEEPASSXC_BROKEN_HEADER_SHA256,
    );
    assert!(open_database(bytes.as_slice(), &key("")).is_err());
}
