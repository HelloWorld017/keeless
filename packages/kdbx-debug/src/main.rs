use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use keeless_kdbx::{
    diagnose_database, CompositeKey, DatabaseError, DiagnosticFailure, DiagnosticOptions,
    DiagnosticReport, DiagnosticStage, DiagnosticStageStatus,
};
use serde::Serialize;
use zeroize::Zeroizing;

#[derive(Debug, Parser)]
#[command(
    name = "kdbx-debug",
    about = "Diagnose why a KeePass KDBX database cannot be opened"
)]
struct Args {
    /// KDBX database to diagnose
    database: PathBuf,

    /// KeePass key file (raw, hex, XML v1, or XML v2)
    #[arg(long, value_name = "PATH")]
    key_file: Option<PathBuf>,

    /// Do not add a password component (requires --key-file)
    #[arg(long, requires = "key_file")]
    no_password: bool,

    /// Emit a single JSON document
    #[arg(long)]
    json: bool,

    /// Dump outer-decrypted XML; protected values stay encrypted
    #[arg(long, value_name = "PATH")]
    dump_xml: Option<PathBuf>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileOutput {
    path: String,
    size_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FailureOutput {
    stage: Option<DiagnosticStage>,
    class: &'static str,
    message: String,
    hints: Vec<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonOutput<'a> {
    status: &'static str,
    file: &'a FileOutput,
    report: &'a DiagnosticReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<FailureOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    xml_dump: Option<String>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) | Err(_) => ExitCode::FAILURE,
    }
}

fn run(args: Args) -> Result<bool, ()> {
    let file = match File::open(&args.database) {
        Ok(file) => file,
        Err(error) => return setup_error(&args, format!("cannot open database: {error}")),
    };
    let size_bytes = match file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(error) => return setup_error(&args, format!("cannot inspect database: {error}")),
    };
    let file_output = FileOutput {
        path: args.database.to_string_lossy().into_owned(),
        size_bytes,
    };

    let mut key = CompositeKey::new();
    if !args.no_password {
        let password = match rpassword::prompt_password("Password: ") {
            Ok(password) => Zeroizing::new(password),
            Err(error) => return setup_error(&args, format!("cannot read password: {error}")),
        };
        key = match key.with_password(password.as_bytes()) {
            Ok(key) => key,
            Err(error) => return setup_error(&args, error.to_string()),
        };
    }
    if let Some(path) = &args.key_file {
        let contents = match std::fs::read(path) {
            Ok(contents) => Zeroizing::new(contents),
            Err(error) => return setup_error(&args, format!("cannot read key file: {error}")),
        };
        key = match key.with_key_file_contents(contents.as_slice()) {
            Ok(key) => key,
            Err(error) => return setup_error(&args, error.to_string()),
        };
    }

    let mut xml_file = args.dump_xml.as_ref().map(|path| LazyXmlFile::new(path));
    if args.dump_xml.is_some() && !args.json {
        eprintln!(
            "warning: XML may contain unprotected database metadata; protected values remain encrypted"
        );
    }
    let options = match xml_file.as_mut() {
        Some(output) => DiagnosticOptions::new().with_xml_output(output),
        None => DiagnosticOptions::new(),
    };

    match diagnose_database(file, &key, options) {
        Ok(success) => {
            if args.json {
                write_json(JsonOutput {
                    status: "ok",
                    file: &file_output,
                    report: &success.report,
                    failure: None,
                    xml_dump: xml_dump_path(&args, &success.report),
                })?;
            } else {
                write_human(
                    &file_output,
                    &success.report,
                    None,
                    args.dump_xml.as_deref(),
                );
            }
            Ok(true)
        }
        Err(failure) => {
            let failure_output = describe_failure(&failure);
            if args.json {
                write_json(JsonOutput {
                    status: "error",
                    file: &file_output,
                    report: &failure.report,
                    failure: Some(failure_output),
                    xml_dump: xml_dump_path(&args, &failure.report),
                })?;
            } else {
                write_human(
                    &file_output,
                    &failure.report,
                    Some(&failure_output),
                    args.dump_xml.as_deref(),
                );
            }
            Ok(false)
        }
    }
}

fn setup_error(args: &Args, message: String) -> Result<bool, ()> {
    if args.json {
        let value = serde_json::json!({
            "status": "error",
            "failure": {
                "stage": null,
                "class": "setup",
                "message": message,
                "hints": []
            }
        });
        serde_json::to_writer_pretty(io::stdout().lock(), &value).map_err(|error| {
            eprintln!("error: cannot write JSON: {error}");
        })?;
        println!();
    } else {
        eprintln!("error: {message}");
    }
    Err(())
}

fn write_json(output: JsonOutput<'_>) -> Result<(), ()> {
    serde_json::to_writer_pretty(io::stdout().lock(), &output).map_err(|error| {
        eprintln!("error: cannot write JSON: {error}");
    })?;
    println!();
    Ok(())
}

fn write_human(
    file: &FileOutput,
    report: &DiagnosticReport,
    failure: Option<&FailureOutput>,
    requested_xml_path: Option<&Path>,
) {
    println!("KDBX diagnostic: {} ({} bytes)", file.path, file.size_bytes);
    if let Some(format) = &report.format {
        println!(
            "Format: {} {}",
            format.format.to_uppercase(),
            format.version
        );
        if let Some(cipher) = &format.cipher {
            println!("Cipher: {cipher}");
        }
        if let Some(compression) = &format.compression {
            println!("Compression: {compression}");
        }
        if let Some(kdf) = &format.kdf {
            let mut parameters = Vec::new();
            if let Some(rounds) = kdf.rounds {
                parameters.push(format!("rounds={rounds}"));
            }
            if let Some(memory) = kdf.memory_bytes {
                parameters.push(format!("memory={memory} bytes"));
            }
            if let Some(parallelism) = kdf.parallelism {
                parameters.push(format!("parallelism={parallelism}"));
            }
            let suffix = if parameters.is_empty() {
                String::new()
            } else {
                format!(" ({})", parameters.join(", "))
            };
            println!("KDF: {}{suffix}", kdf.algorithm);
        }
    }
    println!();
    for step in &report.steps {
        let status = match step.status {
            DiagnosticStageStatus::Ok => "ok",
            DiagnosticStageStatus::Failed => "failed",
        };
        let detail = step
            .detail
            .as_deref()
            .map(|detail| format!(" - {detail}"))
            .unwrap_or_default();
        println!(
            "[{status:6}] {:25} {:>6} ms{detail}",
            step.stage, step.elapsed_ms
        );
    }

    if report.xml_written {
        if let Some(path) = requested_xml_path {
            println!("\nXML written to {}", path.display());
        }
    }
    if let Some(summary) = &report.summary {
        println!(
            "\nResult: database opened ({} groups, {} entries)",
            summary.groups, summary.entries
        );
    } else if let Some(failure) = failure {
        println!("\nResult: failed [{}]: {}", failure.class, failure.message);
        for hint in &failure.hints {
            println!("Hint: {hint}");
        }
    }
}

fn describe_failure(failure: &DiagnosticFailure) -> FailureOutput {
    let stage = failure
        .report
        .steps
        .iter()
        .rev()
        .find(|step| step.status == DiagnosticStageStatus::Failed)
        .map(|step| step.stage);
    let class = error_class(failure.error.as_ref());
    let hints = match stage {
        Some(DiagnosticStage::Signature | DiagnosticStage::Version) => vec![
            "Confirm that the input is an intact KDBX 3.1, 4.0, or 4.1 file.",
        ],
        Some(DiagnosticStage::OuterHeader | DiagnosticStage::HeaderHash) => vec![
            "The file header is truncated, malformed, or corrupted; credentials are not yet involved.",
        ],
        Some(
            DiagnosticStage::HeaderAuthentication
            | DiagnosticStage::CredentialAuthentication,
        ) => vec![
            "Verify the password and key file. Header corruption can produce the same symptom.",
        ],
        Some(DiagnosticStage::PayloadIntegrity) => vec![
            "The authenticated payload is truncated or corrupted. Restore another file version or backup.",
        ],
        Some(DiagnosticStage::XmlParse | DiagnosticStage::ModelValidation) => vec![
            "Cryptography succeeded; inspect --dump-xml output for malformed or unsupported database data.",
        ],
        Some(DiagnosticStage::XmlOutput) => vec![
            "Choose a new writable --dump-xml path; existing files are never overwritten.",
        ],
        _ => Vec::new(),
    };
    FailureOutput {
        stage,
        class,
        message: failure.error.to_string(),
        hints,
    }
}

fn error_class(error: &DatabaseError) -> &'static str {
    match error {
        DatabaseError::Io(_) | DatabaseError::FileNotFound(_) => "io",
        DatabaseError::InvalidSignature(_) => "invalid_signature",
        DatabaseError::InvalidVersion(_) => "invalid_version",
        DatabaseError::InvalidCredentials | DatabaseError::InvalidKey => "invalid_credentials",
        DatabaseError::IntegrityError(_) => "integrity",
        DatabaseError::DecryptionError(_) => "decryption",
        DatabaseError::CompressionError(_) => "compression",
        DatabaseError::XmlError(_) | DatabaseError::QuickXmlError(_) => "xml",
        DatabaseError::SecureMemory(_) => "secure_memory",
        DatabaseError::Unsupported(_) => "unsupported",
        DatabaseError::InvalidFormat(_) => "invalid_format",
        DatabaseError::CryptoError(_) => "crypto",
        DatabaseError::EncryptionError(_) => "encryption",
        DatabaseError::NotLoaded
        | DatabaseError::AlreadyLoaded
        | DatabaseError::MergeError(_)
        | DatabaseError::SearchError(_) => "database",
    }
}

fn xml_dump_path(args: &Args, report: &DiagnosticReport) -> Option<String> {
    report
        .xml_written
        .then(|| {
            args.dump_xml
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
        })
        .flatten()
}

struct LazyXmlFile {
    path: PathBuf,
    file: Option<File>,
}

impl LazyXmlFile {
    fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            file: None,
        }
    }

    fn open(&mut self) -> io::Result<&mut File> {
        if self.file.is_none() {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            self.file = Some(options.open(&self.path)?);
        }
        Ok(self.file.as_mut().expect("XML file was initialized"))
    }
}

impl Write for LazyXmlFile {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.open()?.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_failure_has_actionable_hint() {
        let failure = DiagnosticFailure {
            error: Box::new(DatabaseError::InvalidCredentials),
            report: Box::new(DiagnosticReport {
                steps: vec![keeless_kdbx::DiagnosticStep {
                    stage: DiagnosticStage::HeaderAuthentication,
                    status: DiagnosticStageStatus::Failed,
                    elapsed_ms: 1,
                    detail: None,
                }],
                ..DiagnosticReport::default()
            }),
        };
        let output = describe_failure(&failure);
        assert_eq!(output.class, "invalid_credentials");
        assert_eq!(output.stage, Some(DiagnosticStage::HeaderAuthentication));
        assert!(!output.hints.is_empty());
    }
}
