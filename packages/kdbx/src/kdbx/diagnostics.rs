//! Structured diagnostics for the KDBX open pipeline.

use std::fmt;
use std::io::Write;
use std::time::Instant;

use serde::Serialize;

use crate::crypto::CompressionAlgorithm;
use crate::kdbx::file::header::{KdbxHeader31, KdbxHeader4};
use crate::kdbx::kdf::aes_kdf::AES_KDF_UUID;
use crate::kdbx::kdf::argon2_kdf::{ARGON2D_UUID, ARGON2ID_UUID};
use crate::model::{Database, DatabaseVersion};
use crate::{DatabaseError, DatabaseResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticStage {
    Signature,
    Version,
    OuterHeader,
    HeaderHash,
    KeyDerivation,
    HeaderAuthentication,
    PayloadRead,
    PayloadIntegrity,
    Decryption,
    CredentialAuthentication,
    Decompression,
    InnerHeader,
    InnerProtection,
    XmlOutput,
    XmlParse,
    ModelValidation,
    MemoryProtection,
}

impl DiagnosticStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Signature => "signature",
            Self::Version => "version",
            Self::OuterHeader => "outer_header",
            Self::HeaderHash => "header_hash",
            Self::KeyDerivation => "key_derivation",
            Self::HeaderAuthentication => "header_authentication",
            Self::PayloadRead => "payload_read",
            Self::PayloadIntegrity => "payload_integrity",
            Self::Decryption => "decryption",
            Self::CredentialAuthentication => "credential_authentication",
            Self::Decompression => "decompression",
            Self::InnerHeader => "inner_header",
            Self::InnerProtection => "inner_protection",
            Self::XmlOutput => "xml_output",
            Self::XmlParse => "xml_parse",
            Self::ModelValidation => "model_validation",
            Self::MemoryProtection => "memory_protection",
        }
    }
}

impl fmt::Display for DiagnosticStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticStageStatus {
    Ok,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticStep {
    pub stage: DiagnosticStage,
    pub status: DiagnosticStageStatus,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KdfDiagnostic {
    pub algorithm: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rounds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallelism: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatDiagnostic {
    pub format: String,
    pub version: String,
    pub raw_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cipher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compression: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kdf: Option<KdfDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseDiagnosticSummary {
    pub groups: usize,
    pub entries: usize,
    pub deleted_objects: usize,
    pub custom_icons: usize,
    pub contains_unsupported_xml: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<FormatDiagnostic>,
    pub steps: Vec<DiagnosticStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<DatabaseDiagnosticSummary>,
    pub xml_written: bool,
}

pub struct DiagnosticOptions<'a> {
    xml_output: Option<&'a mut dyn Write>,
}

impl<'a> DiagnosticOptions<'a> {
    pub fn new() -> Self {
        Self { xml_output: None }
    }

    /// Write the outer-decrypted XML while retaining `Protected="True"` values.
    pub fn with_xml_output(mut self, output: &'a mut dyn Write) -> Self {
        self.xml_output = Some(output);
        self
    }
}

impl Default for DiagnosticOptions<'_> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct DiagnosticSuccess {
    pub database: Database,
    pub report: DiagnosticReport,
}

#[derive(Debug)]
pub struct DiagnosticFailure {
    pub error: Box<DatabaseError>,
    pub report: Box<DiagnosticReport>,
}

impl fmt::Display for DiagnosticFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl std::error::Error for DiagnosticFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.error.as_ref())
    }
}

pub(crate) struct DiagnosticContext<'a> {
    enabled: bool,
    report: DiagnosticReport,
    xml_output: Option<&'a mut dyn Write>,
}

impl DiagnosticContext<'_> {
    pub(crate) fn disabled() -> Self {
        Self {
            enabled: false,
            report: DiagnosticReport::default(),
            xml_output: None,
        }
    }
}

impl<'a> DiagnosticContext<'a> {
    pub(crate) fn enabled(options: DiagnosticOptions<'a>) -> Self {
        Self {
            enabled: true,
            report: DiagnosticReport::default(),
            xml_output: options.xml_output,
        }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn run<T>(
        &mut self,
        stage: DiagnosticStage,
        operation: impl FnOnce() -> DatabaseResult<T>,
        detail: impl FnOnce(&T) -> Option<String>,
    ) -> DatabaseResult<T> {
        if !self.enabled {
            return operation();
        }

        let started = Instant::now();
        match operation() {
            Ok(value) => {
                self.report.steps.push(DiagnosticStep {
                    stage,
                    status: DiagnosticStageStatus::Ok,
                    elapsed_ms: elapsed_millis(started),
                    detail: detail(&value),
                });
                Ok(value)
            }
            Err(error) => {
                self.report.steps.push(DiagnosticStep {
                    stage,
                    status: DiagnosticStageStatus::Failed,
                    elapsed_ms: elapsed_millis(started),
                    detail: None,
                });
                Err(error)
            }
        }
    }

    pub(crate) fn set_version(&mut self, version: DatabaseVersion, raw_version: u32) {
        if !self.enabled {
            return;
        }
        let version_text = format!("{}.{}", raw_version >> 16, raw_version & 0xffff);
        self.report.format = Some(FormatDiagnostic {
            format: match version {
                DatabaseVersion::KDB => "kdb",
                DatabaseVersion::KDBX31 | DatabaseVersion::KDBX4 => "kdbx",
            }
            .into(),
            version: version_text,
            raw_version,
            cipher: None,
            compression: None,
            kdf: None,
        });
    }

    pub(crate) fn set_kdbx31_header(&mut self, header: &KdbxHeader31) {
        if let Some(format) = self.report.format.as_mut() {
            format.cipher = Some(header.encryption_algorithm.name().into());
            format.compression = Some(compression_name(header.compression).into());
            format.kdf = Some(KdfDiagnostic {
                algorithm: "aes-kdf".into(),
                rounds: Some(header.transform_rounds),
                memory_bytes: None,
                parallelism: None,
                version: None,
            });
        }
    }

    pub(crate) fn set_kdbx4_header(&mut self, header: &KdbxHeader4) {
        if let Some(format) = self.report.format.as_mut() {
            format.cipher = Some(header.encryption_algorithm.name().into());
            format.compression = Some(compression_name(header.compression).into());
            format.kdf = header.kdf_parameters.as_ref().map(|params| {
                let algorithm = match params.kdf_uuid {
                    AES_KDF_UUID => "aes-kdf",
                    ARGON2D_UUID => "argon2d",
                    ARGON2ID_UUID => "argon2id",
                    _ => "unknown",
                };
                KdfDiagnostic {
                    algorithm: algorithm.into(),
                    rounds: params.get_uint64("R").or_else(|| params.get_uint64("I")),
                    memory_bytes: params.get_uint64("M"),
                    parallelism: params.get_uint32("P"),
                    version: params.get_uint32("V"),
                }
            });
        }
    }

    pub(crate) fn write_xml(&mut self, xml: &[u8]) -> DatabaseResult<()> {
        let Some(output) = self.xml_output.as_mut() else {
            return Ok(());
        };
        let started = Instant::now();
        match output.write_all(xml) {
            Ok(()) => {
                self.report.xml_written = true;
                self.report.steps.push(DiagnosticStep {
                    stage: DiagnosticStage::XmlOutput,
                    status: DiagnosticStageStatus::Ok,
                    elapsed_ms: elapsed_millis(started),
                    detail: Some(format!("{} bytes", xml.len())),
                });
                Ok(())
            }
            Err(error) => {
                self.report.steps.push(DiagnosticStep {
                    stage: DiagnosticStage::XmlOutput,
                    status: DiagnosticStageStatus::Failed,
                    elapsed_ms: elapsed_millis(started),
                    detail: None,
                });
                Err(error.into())
            }
        }
    }

    pub(crate) fn set_summary(&mut self, database: &Database) {
        if self.enabled {
            self.report.summary = Some(DatabaseDiagnosticSummary {
                groups: database.groups.len(),
                entries: database.entries.len(),
                deleted_objects: database.deleted_objects.len(),
                custom_icons: database.custom_icons.len(),
                contains_unsupported_xml: database.contains_unsupported_xml,
            });
        }
    }

    pub(crate) fn into_report(self) -> DiagnosticReport {
        self.report
    }
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn compression_name(compression: CompressionAlgorithm) -> &'static str {
    match compression {
        CompressionAlgorithm::None => "none",
        CompressionAlgorithm::Gzip => "gzip",
    }
}
