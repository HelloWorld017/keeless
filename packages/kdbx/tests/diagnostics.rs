use std::fs::File;
use std::path::{Path, PathBuf};

use keeless_kdbx::{
    diagnose_database, CompositeCredentials, DiagnosticOptions, DiagnosticStage,
    DiagnosticStageStatus,
};

fn resource(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/resources")
        .join(path)
}

fn password(value: &str) -> CompositeCredentials {
    CompositeCredentials::new()
        .with_password(value.as_bytes())
        .unwrap()
}

#[test]
fn reports_successful_kdbx4_pipeline_and_safe_metadata() {
    let result = diagnose_database(
        File::open(resource("test_db_kdbx4_with_password_argon2id.kdbx")).unwrap(),
        &password("demopass"),
        DiagnosticOptions::new(),
    )
    .unwrap();

    let format = result.report.format.unwrap();
    assert_eq!(format.format, "kdbx");
    assert_eq!(format.version, "4.0");
    assert_eq!(format.kdf.unwrap().algorithm, "argon2id");
    assert!(result.report.summary.is_some());
    assert!(result
        .report
        .steps
        .iter()
        .all(|step| { step.status == DiagnosticStageStatus::Ok }));
    assert!(result
        .report
        .steps
        .iter()
        .any(|step| step.stage == DiagnosticStage::ModelValidation));
}

#[test]
fn locates_wrong_password_at_header_authentication() {
    let failure = diagnose_database(
        File::open(resource("upstream/keepassxc/Format400.kdbx")).unwrap(),
        &password("wrong"),
        DiagnosticOptions::new(),
    )
    .unwrap_err();

    let failed = failure.report.steps.last().unwrap();
    assert_eq!(failed.stage, DiagnosticStage::HeaderAuthentication);
    assert_eq!(failed.status, DiagnosticStageStatus::Failed);
}

#[test]
fn locates_broken_header_before_key_derivation() {
    let mut bytes = std::fs::read(resource("test_db_kdbx4_with_password_aes.kdbx")).unwrap();
    let mut position = 12;
    loop {
        let field_id = bytes[position];
        let field_size =
            u32::from_le_bytes(bytes[position + 1..position + 5].try_into().unwrap()) as usize;
        position += 5 + field_size;
        if field_id == 0 {
            break;
        }
    }
    bytes[position] ^= 0x01;

    let failure = diagnose_database(
        bytes.as_slice(),
        &password("demopass"),
        DiagnosticOptions::new(),
    )
    .unwrap_err();

    let failed = failure.report.steps.last().unwrap();
    assert_eq!(failed.stage, DiagnosticStage::HeaderHash);
    assert!(!failure
        .report
        .steps
        .iter()
        .any(|step| step.stage == DiagnosticStage::KeyDerivation));
}

#[test]
fn xml_dump_keeps_protected_values_encrypted() {
    let mut xml = Vec::new();
    let result = diagnose_database(
        File::open(resource("upstream/keepassxc/ProtectedStrings.kdbx")).unwrap(),
        &password("masterpw"),
        DiagnosticOptions::new().with_xml_output(&mut xml),
    )
    .unwrap();

    let text = std::str::from_utf8(&xml).unwrap();
    assert!(result.report.xml_written);
    assert!(text.contains("Protected=\"True\""));
    assert!(!text.contains("ProtectedPassword"));
}

#[test]
fn reports_successful_kdbx31_pipeline() {
    let result = diagnose_database(
        File::open(resource("upstream/keepassxc/Compressed.kdbx")).unwrap(),
        &password(""),
        DiagnosticOptions::new(),
    )
    .unwrap();

    assert_eq!(result.report.format.unwrap().version, "3.1");
    assert!(result
        .report
        .steps
        .iter()
        .any(|step| step.stage == DiagnosticStage::CredentialAuthentication));
}
