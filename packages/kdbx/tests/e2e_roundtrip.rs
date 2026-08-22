//! End-to-end integration tests
//!
//! Tests the complete write → read roundtrip pipeline for all 3 formats
//! using realistic database data with entries, groups, and attachments.

use std::io::Cursor;

use base64::Engine;
use keeless_kdbx::kdbx::kdf::argon2_kdf::ARGON2ID_UUID;
use keeless_kdbx::kdbx::kdf::create_kdf;
use keeless_kdbx::model::core::node::NodeId;
use keeless_kdbx::model::core::security::ProtectedString;
use keeless_kdbx::model::db::composite_key::{CompositeCredentials, CompositeKey};
use keeless_kdbx::model::db::database::{Database, DatabaseVersion};
use keeless_kdbx::model::entry::Entry;
use keeless_kdbx::model::group::Group;
use keeless_kdbx::{diagnose_database, initialize_database_key, DatabaseError, DiagnosticOptions};
use sha2::{Digest, Sha256};

/// Build a realistic test database with multiple groups and entries.
fn build_realistic_database() -> Database {
    let mut db = Database::new(DatabaseVersion::KDBX4);
    db.name = "Test Database".to_string();
    db.description = "A realistic test database".to_string();
    db.default_username = "testuser".to_string();

    // Root group
    let root_id = NodeId::new_uuid();
    let mut root = Group::new(root_id);
    root.title = "Root".to_string();

    // General group
    let general_id = NodeId::new_uuid();
    let mut general = Group::new(general_id);
    general.title = "General".to_string();
    root.add_child_group(general_id);

    // Email group
    let email_id = NodeId::new_uuid();
    let mut email = Group::new(email_id);
    email.title = "Email Accounts".to_string();
    root.add_child_group(email_id);

    // Nested group
    let work_id = NodeId::new_uuid();
    let mut work = Group::new(work_id);
    work.title = "Work".to_string();
    email.add_child_group(work_id);

    // Entry 1: Gmail
    let e1_id = NodeId::new_uuid();
    let mut e1 = Entry::new(e1_id);
    e1.set_title("Gmail Account");
    e1.set_username(ProtectedString::new_protected("user@gmail.com"));
    e1.set_password(ProtectedString::new_protected("super_secret_password"));
    e1.set_url("https://mail.google.com");
    e1.set_notes(ProtectedString::new_protected(
        "Personal email with 2FA enabled",
    ));
    general.add_child_entry(e1_id);

    // Entry 2: GitHub
    let e2_id = NodeId::new_uuid();
    let mut e2 = Entry::new(e2_id);
    e2.set_title("GitHub");
    e2.set_username(ProtectedString::new_protected("developer"));
    e2.set_password(ProtectedString::new_protected("ghp_abc123def456"));
    e2.set_url("https://github.com");
    e2.tags = vec!["development".to_string(), "coding".to_string()];
    general.add_child_entry(e2_id);

    // Entry 3: AWS (with custom fields)
    let e3_id = NodeId::new_uuid();
    let mut e3 = Entry::new(e3_id);
    e3.set_title("AWS Console");
    e3.set_username(ProtectedString::new_protected("admin@company.com"));
    e3.set_password(ProtectedString::new_protected("aws_secret_key_123"));
    e3.set_url("https://console.aws.amazon.com");
    e3.set_notes(ProtectedString::new_protected(
        "Root account — use IAM for daily work",
    ));
    email.add_child_entry(e3_id);

    // Entry 4: Work VPN
    let e4_id = NodeId::new_uuid();
    let mut e4 = Entry::new(e4_id);
    e4.set_title("Work VPN");
    e4.set_username(ProtectedString::new_protected("jdoe"));
    e4.set_password(ProtectedString::new_protected("vpn_password"));
    e4.set_url("vpn.company.com");
    work.add_child_entry(e4_id);

    // Insert everything
    db.groups.insert(root_id, root);
    db.groups.insert(general_id, general);
    db.groups.insert(email_id, email);
    db.groups.insert(work_id, work);
    db.entries.insert(e1_id, e1);
    db.entries.insert(e2_id, e2);
    db.entries.insert(e3_id, e3);
    db.entries.insert(e4_id, e4);

    db.root_group_id = Some(root_id);
    db
}

fn make_key(database: &mut Database) -> CompositeKey {
    let credentials = CompositeCredentials::new()
        .with_password(b"integration_test_password_2024")
        .unwrap();
    match database.kdf_parameters.as_ref() {
        Some(parameters) => credentials.derive_key(parameters).unwrap(),
        None => initialize_database_key(database, &credentials).unwrap(),
    }
}

fn kdbx31_header_end(bytes: &[u8]) -> usize {
    let mut offset = 12;
    loop {
        assert!(offset + 3 <= bytes.len(), "truncated KDBX 3.1 header");
        let field_id = bytes[offset];
        let field_len = u16::from_le_bytes([bytes[offset + 1], bytes[offset + 2]]) as usize;
        offset += 3;
        assert!(
            offset + field_len <= bytes.len(),
            "truncated KDBX 3.1 field"
        );
        offset += field_len;
        if field_id == 0 {
            return offset;
        }
    }
}

/// Verify that a round-tripped database preserved key data.
fn verify_database(db: &Database) {
    assert!(db.root_group().is_some(), "Root group should exist");
    assert!(
        db.groups.len() >= 3,
        "Should have at least 3 groups, got {}",
        db.groups.len()
    );
    assert!(
        db.entries.len() >= 3,
        "Should have at least 3 entries, got {}",
        db.entries.len()
    );

    // Verify entries have content
    let mut found_gmail = false;
    let mut found_github = false;
    for entry in db.entries.values() {
        if entry.title().as_str().contains("Gmail") {
            found_gmail = true;
            assert!(!entry.username().as_str().is_empty());
            assert!(!entry.password().as_str().is_empty());
        }
        if entry.title().as_str().contains("GitHub") {
            found_github = true;
            assert_eq!(entry.tags.len(), 2);
        }
    }
    assert!(found_gmail, "Gmail entry should be preserved");
    assert!(found_github, "GitHub entry should be preserved");
}

// ── KDBX 3.1 end-to-end ──

#[test]
fn test_e2e_kdbx31_roundtrip() {
    let mut db = build_realistic_database();
    db.version = DatabaseVersion::KDBX31;
    db.encryption_algorithm =
        keeless_kdbx::crypto::encryption_algorithm::EncryptionAlgorithm::AesRijndael;
    db.compression = keeless_kdbx::crypto::compression::CompressionAlgorithm::Gzip;

    let key = make_key(&mut db);

    // Write
    let mut buffer = Vec::new();
    keeless_kdbx::kdbx::file::kdbx31_writer::write_kdbx31(&mut buffer, &db, &key)
        .expect("KDBX 3.1 write should succeed");

    assert!(buffer.len() > 100, "Written data should be substantial");

    // Read back
    let mut cursor = Cursor::new(buffer);
    let loaded = keeless_kdbx::kdbx::file::kdbx31_reader::read_kdbx31(&mut cursor, &key)
        .expect("KDBX 3.1 read should succeed");

    verify_database(&loaded);
}

#[test]
fn test_e2e_kdbx31_header_hash_matches_outer_header() {
    let mut db = build_realistic_database();
    db.version = DatabaseVersion::KDBX31;
    let key = make_key(&mut db);

    let mut encoded = Vec::new();
    keeless_kdbx::kdbx::file::kdbx31_writer::write_kdbx31(&mut encoded, &db, &key)
        .expect("KDBX 3.1 write should succeed");

    let header_end = kdbx31_header_end(&encoded);
    let expected_hash =
        base64::engine::general_purpose::STANDARD.encode(Sha256::digest(&encoded[..header_end]));
    let mut xml = Vec::new();
    diagnose_database(
        Cursor::new(&encoded),
        &CompositeCredentials::new()
            .with_password(b"integration_test_password_2024")
            .unwrap(),
        DiagnosticOptions::new().with_xml_output(&mut xml),
    )
    .expect("KDBX 3.1 output should pass header hash verification");
    let xml = std::str::from_utf8(&xml).expect("diagnostic XML should be UTF-8");

    assert!(xml.contains(&format!("<HeaderHash>{expected_hash}</HeaderHash>")));
}

#[test]
fn test_e2e_kdbx31_rejects_outer_header_hash_mismatch() {
    let mut db = build_realistic_database();
    db.version = DatabaseVersion::KDBX31;
    let key = make_key(&mut db);

    let mut encoded = Vec::new();
    keeless_kdbx::kdbx::file::kdbx31_writer::write_kdbx31(&mut encoded, &db, &key)
        .expect("KDBX 3.1 write should succeed");

    // Add an ignored outer-header comment. This keeps decryption valid while
    // changing the header bytes covered by Meta/HeaderHash.
    let header_end = kdbx31_header_end(&encoded);
    let end_field_start = header_end - 7;
    encoded.splice(end_field_start..end_field_start, [1, 0, 0]);

    let error =
        keeless_kdbx::kdbx::file::kdbx31_reader::read_kdbx31(&mut Cursor::new(encoded), &key)
            .expect_err("header hash mismatch should be rejected");
    assert!(matches!(
        error,
        DatabaseError::InvalidFormat(message) if message == "Header hash mismatch"
    ));
}

// ── KDBX 3.1 with wrong password ──

#[test]
fn test_e2e_kdbx31_wrong_password() {
    let _db = build_realistic_database();
    let mut db = build_realistic_database();
    db.version = DatabaseVersion::KDBX31;

    let key = make_key(&mut db);

    let mut buffer = Vec::new();
    keeless_kdbx::kdbx::file::kdbx31_writer::write_kdbx31(&mut buffer, &db, &key)
        .expect("Write should succeed");

    // Read with wrong password
    let wrong_key = CompositeCredentials::new()
        .with_password(b"wrong_password")
        .unwrap()
        .derive_key(db.kdf_parameters.as_ref().unwrap())
        .unwrap();
    let mut cursor = Cursor::new(buffer);
    let result = keeless_kdbx::kdbx::file::kdbx31_reader::read_kdbx31(&mut cursor, &wrong_key);
    assert!(result.is_err(), "Wrong password should fail");
}

// ── KDBX 3.1 with ChaCha20 ──

#[test]
fn test_e2e_kdbx31_chacha20() {
    let _db = build_realistic_database();
    let mut db = build_realistic_database();
    db.version = DatabaseVersion::KDBX31;
    db.encryption_algorithm =
        keeless_kdbx::crypto::encryption_algorithm::EncryptionAlgorithm::ChaCha20;

    let key = make_key(&mut db);

    let mut buffer = Vec::new();
    keeless_kdbx::kdbx::file::kdbx31_writer::write_kdbx31(&mut buffer, &db, &key)
        .expect("ChaCha20 write should succeed");

    let mut cursor = Cursor::new(buffer);
    let loaded = keeless_kdbx::kdbx::file::kdbx31_reader::read_kdbx31(&mut cursor, &key)
        .expect("ChaCha20 read should succeed");

    verify_database(&loaded);
}

// ── KDBX 4.0 end-to-end ──

#[test]
fn test_e2e_kdbx4_roundtrip() {
    let mut db = build_realistic_database();
    db.version = DatabaseVersion::KDBX4;
    db.encryption_algorithm =
        keeless_kdbx::crypto::encryption_algorithm::EncryptionAlgorithm::AesRijndael;

    // Use minimal Argon2 parameters for test speed
    let kdf = create_kdf(&ARGON2ID_UUID).expect("Argon2 KDF");
    let mut params = kdf.default_parameters();
    params.set_uint64("M", 64 * 1024); // 64 KB instead of 16 MB
    params.set_uint64("I", 1); // 1 iteration
    kdf.randomize(&mut params).expect("Argon2 salt");
    db.kdf_parameters = Some(params);

    let key = make_key(&mut db);

    // Write
    let mut buffer = Vec::new();
    keeless_kdbx::kdbx::file::kdbx4_writer::write_kdbx4(&mut buffer, &db, &key)
        .expect("KDBX 4.0 write should succeed");

    assert!(buffer.len() > 100);

    // Read back
    let mut cursor = Cursor::new(buffer);
    let loaded = keeless_kdbx::kdbx::file::kdbx4_reader::read_kdbx4(&mut cursor, &key)
        .expect("KDBX 4.0 read should succeed");

    verify_database(&loaded);
}

// ── KDB end-to-end ──

#[test]
fn test_e2e_kdb_roundtrip() {
    let _db = build_realistic_database();
    let mut db = build_realistic_database();
    db.version = DatabaseVersion::KDB;

    let key = make_key(&mut db);

    // Write
    let mut buffer = Vec::new();
    keeless_kdbx::kdbx::file::kdb_writer::write_kdb(&mut buffer, &db, &key)
        .expect("KDB write should succeed");

    assert!(buffer.len() > 100);

    // Read back — need to detect version first to consume signature
    let mut cursor = Cursor::new(buffer);
    let version = keeless_kdbx::kdbx::file::reader::DatabaseReader::detect_version(&mut cursor)
        .expect("Should detect KDB version");
    assert_eq!(version, DatabaseVersion::KDB);

    let loaded = keeless_kdbx::kdbx::file::kdb_reader::read_kdb(&mut cursor, &key)
        .expect("KDB read should succeed");

    // KDB has limited fields — just check groups and entries survived
    assert!(loaded.groups.len() >= 2, "Should have at least 2 groups");
    assert!(loaded.entries.len() >= 2, "Should have at least 2 entries");
}

// ── Empty database roundtrip ──

#[test]
fn test_e2e_kdbx31_empty_database() {
    let mut db = Database::new(DatabaseVersion::KDBX31);
    let root_id = NodeId::new_uuid();
    let mut root = Group::new(root_id);
    root.title = "Root".to_string();
    db.groups.insert(root_id, root);
    db.root_group_id = Some(root_id);

    let key = make_key(&mut db);

    let mut buffer = Vec::new();
    keeless_kdbx::kdbx::file::kdbx31_writer::write_kdbx31(&mut buffer, &db, &key)
        .expect("Empty DB write should succeed");

    let mut cursor = Cursor::new(buffer);
    let loaded = keeless_kdbx::kdbx::file::kdbx31_reader::read_kdbx31(&mut cursor, &key)
        .expect("Empty DB read should succeed");

    assert!(loaded.root_group().is_some());
    assert_eq!(loaded.entries.len(), 0);
}

// ── Unicode content roundtrip ──

#[test]
fn test_e2e_kdbx31_unicode_content() {
    let mut db = Database::new(DatabaseVersion::KDBX31);
    let root_id = NodeId::new_uuid();
    let mut root = Group::new(root_id);
    root.title = "根目录".to_string();

    let entry_id = NodeId::new_uuid();
    let mut entry = Entry::new(entry_id);
    entry.set_title("日本語エントリ");
    entry.set_username(ProtectedString::new_protected("用户名"));
    entry.set_password(ProtectedString::new_protected("密码密码"));
    entry.set_url("https://例え.jp");
    entry.set_notes(ProtectedString::new_protected("备注信息 🔐"));
    root.add_child_entry(entry_id);

    db.groups.insert(root_id, root);
    db.entries.insert(entry_id, entry);
    db.root_group_id = Some(root_id);

    let key = make_key(&mut db);

    let mut buffer = Vec::new();
    keeless_kdbx::kdbx::file::kdbx31_writer::write_kdbx31(&mut buffer, &db, &key)
        .expect("Unicode write should succeed");

    let mut cursor = Cursor::new(buffer);
    let loaded = keeless_kdbx::kdbx::file::kdbx31_reader::read_kdbx31(&mut cursor, &key)
        .expect("Unicode read should succeed");

    let loaded_entry = loaded.entries.values().next().unwrap();
    assert_eq!(loaded_entry.title().as_str(), "日本語エントリ");
    assert_eq!(loaded_entry.username().as_str(), "用户名");
    assert!(loaded_entry.notes().as_str().contains("🔐"));
}

// ── P2-1 regression: empty groups must survive full KDBX roundtrip ──
//
// Previously, writing a KDBX file and reading it back silently dropped
// any child group that contained no entries and no subgroups, because
// `Database` cached the root group as a separate field that went stale.
// The fix moved to a single source of truth (`root_group_id` + `groups`
// map). This test guards against the regression by writing through the
// real KDBX 4 file writer/reader (not just the XML layer).
#[test]
fn test_e2e_kdbx4_empty_subgroup_survives_roundtrip() {
    let mut db = Database::new(DatabaseVersion::KDBX4);

    let root_id = NodeId::new_uuid();
    let mut root = Group::new(root_id);
    root.title = "Root".to_string();

    // An empty subgroup — no entries, no children. This is the case that
    // used to disappear.
    let empty_id = NodeId::new_uuid();
    let mut empty_group = Group::new(empty_id);
    empty_group.title = "EmptySub".to_string();
    root.add_child_group(empty_id);

    // A nested empty subgroup under the empty one, to verify recursion.
    let deep_empty_id = NodeId::new_uuid();
    let mut deep_empty = Group::new(deep_empty_id);
    deep_empty.title = "DeepEmpty".to_string();
    empty_group.add_child_group(deep_empty_id);

    db.groups.insert(empty_id, empty_group);
    db.groups.insert(deep_empty_id, deep_empty);
    db.groups.insert(root_id, root);
    db.root_group_id = Some(root_id);

    let key = make_key(&mut db);

    let mut buffer = Vec::new();
    keeless_kdbx::kdbx::file::kdbx4_writer::write_kdbx4(&mut buffer, &db, &key)
        .expect("KDBX4 write should succeed");

    let mut cursor = Cursor::new(buffer);
    let loaded = keeless_kdbx::kdbx::file::kdbx4_reader::read_kdbx4(&mut cursor, &key)
        .expect("KDBX4 read should succeed");

    // root + EmptySub + DeepEmpty = 3 groups
    assert_eq!(
        loaded.groups.len(),
        3,
        "empty subgroups must survive roundtrip"
    );

    let loaded_root = loaded.root_group().expect("root group must exist");
    assert_eq!(
        loaded_root.child_group_ids.len(),
        1,
        "root should have 1 child group"
    );

    let loaded_empty = loaded
        .groups
        .get(&loaded_root.child_group_ids[0])
        .expect("EmptySub must exist");
    assert_eq!(loaded_empty.title, "EmptySub");
    assert!(loaded_empty.child_entry_ids.is_empty());
    assert_eq!(
        loaded_empty.child_group_ids.len(),
        1,
        "EmptySub should still contain DeepEmpty"
    );

    let loaded_deep = loaded
        .groups
        .get(&loaded_empty.child_group_ids[0])
        .expect("DeepEmpty must exist");
    assert_eq!(loaded_deep.title, "DeepEmpty");
    assert!(loaded_deep.child_entry_ids.is_empty());
    assert!(loaded_deep.child_group_ids.is_empty());
}
