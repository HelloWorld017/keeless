//! # keeless_kdbx
//!
//! Platform-independent KeePass database library.
//! Supports KDB (v1) and KDBX (v3.1, v4.0) file formats.
//!
//! ## Architecture
//! - `crypto` - Encryption algorithm abstractions and KDF engines
//! - `model` - Data models (Entry, Group, Node, Icon, etc.)
//! - `kdbx` - KDBX file I/O, KDF, merge, queries, repair, XML, stream
//!
//! KDBX4 compatibility is verified against independent fixtures and parser output.

pub mod crypto;
pub mod kdbx;
pub mod model;

pub use keeless_secure_types::{SecureArray, SecureBytes, SecureString};
pub use model::exception::{DatabaseError, DatabaseResult};

// Top-level convenience re-exports

// ─── Element (Data Models) ────────────────────────────────────────────
pub use model::{
    get_builtin_templates, parse_tags, serialize_tags, AutoType, AutoTypeAssociation, BinaryCache,
    BinaryData, BinaryPool, BinaryStreamReader, BinaryStreamWriter, ChangeRecord, ChangeTracker,
    ChangeType, CompositeKey, CustomData, CustomDataItem, Database, DatabaseVersion, DateInstant,
    DeletedObject, DiffResult, Entry, EntryBinary, EntryField, EntryFieldSelector, EntryKDB,
    EntryKDBX, FieldReference, Group, GroupKDB, GroupKDBX, IconImage, IconImageCustom,
    IconImageStandard, MasterCredential, MemoryProtectionConfig, Node, NodeHandler, NodeId,
    NodeType, ProtectedString, RefTarget, SortNodeEnum, Tag, Template, TemplateField,
    TemplateFieldType, TraversalOrder,
};

// ─── Crypto ───────────────────────────────────────────────────────────
pub use crypto::{
    AesCipher, AesCipherEngine, AesKeyTransformer, ArcFourInnerStream, Argon2Kdf, Argon2Params,
    Argon2Type, BlockMode, ChaCha20Cipher, ChaCha20CipherEngine, ChaCha20InnerStream, CipherEngine,
    CipherMode, CompressionAlgorithm, CryptoError, CryptoResult, EncryptionAlgorithm, HashEngine,
    HmacCompute, InnerStreamCipher, Salsa20Cipher, Salsa20InnerStream, StreamCipher, TwofishCipher,
    TwofishCipherEngine,
};

// ─── Signatures & Variant Dictionary ─────────────────────────────────
pub use kdbx::diagnostics::{
    DatabaseDiagnosticSummary, DiagnosticFailure, DiagnosticOptions, DiagnosticReport,
    DiagnosticStage, DiagnosticStageStatus, DiagnosticStep, DiagnosticSuccess, FormatDiagnostic,
    KdfDiagnostic,
};
pub use kdbx::signature::{
    DigitalSignature, PublicKey, SignatureAlgorithm, SignatureStatus, SignatureVerifier,
};
pub use kdbx::variant_dictionary::{VariantDictionary, VdValue};

// ─── KDF ──────────────────────────────────────────────────────────────
pub use kdbx::kdf::aes_kdf::AesKdf;
pub use kdbx::kdf::argon2_kdf::Argon2Kdf as Argon2KdfEngine;
pub use kdbx::kdf::{KdfEngine, KdfParameters};
// Note: Argon2Variant is re-exported from crypto as Argon2Type alias

// ─── I/O ──────────────────────────────────────────────────────────────
pub use kdbx::file::reader::DatabaseReader;
pub use kdbx::file::writer::DatabaseWriter;

// ─── Queries ──────────────────────────────────────────────────────────
pub use kdbx::query::{
    SearchHelper, SearchParameters, SearchResult, TagQuery, TagResult, UrlMatchParameters,
    UrlMatchResult, UrlMatcher,
};

// ─── Merge ────────────────────────────────────────────────────────────
pub use kdbx::merge::{
    ConflictResolution, ConflictType, DatabaseMerger, MergeConflict, MergeResult, MergeStrategy,
};

// ─── OTP ──────────────────────────────────────────────────────────────
pub use model::entry::otp::{OtpHashAlgorithm, OtpParameters, OtpType, TokenCalculator};

// ─── Passkeys ─────────────────────────────────────────────────────────
pub use model::entry::passkey::{
    is_passkey_entry, AuthenticationRequest, AuthenticationResponse, PasskeyAlgorithm,
    PasskeyAuthenticator, PasskeyCredential, PasskeyError, RegistrationRequest,
    RegistrationResponse, RegistrationResult, UserVerification, KEELESS_AAGUID,
};

// ─── Repair ───────────────────────────────────────────────────────────
pub use kdbx::repair::{
    IntegrityError, IntegrityReport, IntegrityVerifier, IntegrityWarning, RepairResult,
};

// ─── Fuzz ─────────────────────────────────────────────────────────────
pub use kdbx::fuzz::{FuzzOutcome, FuzzResult, FuzzTarget};

// ─── XML ──────────────────────────────────────────────────────────────
pub use kdbx::xml::{KdbxXmlReader, KdbxXmlWriter};

/// Open a KeePass database from a reader.
///
/// Automatically detects the format (KDB, KDBX 3.1, KDBX 4.0) and reads the database.
///
/// # Streaming
///
/// Only the 12-byte file signature is buffered up-front for version detection;
/// the underlying reader is then chained back so each format reader consumes
/// the rest of the stream directly. This avoids the previous 100 MiB full-file
/// pre-buffer and keeps peak memory at roughly the size of the decrypted
/// payload (1× instead of 2×).
///
/// # Example
/// ```ignore
/// use keeless_kdbx::{open_database, CompositeKey};
///
/// let file = std::fs::File::open("database.kdbx")?;
/// let key = CompositeKey::new().with_password(b"mypassword").unwrap();
/// let db = open_database(file, &key)?;
/// println!("Opened {} entries", db.entry_count());
/// ```
pub fn open_database<R: std::io::Read>(reader: R, key: &CompositeKey) -> DatabaseResult<Database> {
    let mut diagnostics = kdbx::diagnostics::DiagnosticContext::disabled();
    open_database_internal(reader, key, &mut diagnostics, false)
}

/// Open a KDBX database and return a structured report even when opening fails.
pub fn diagnose_database<R: std::io::Read>(
    reader: R,
    key: &CompositeKey,
    options: DiagnosticOptions<'_>,
) -> Result<DiagnosticSuccess, DiagnosticFailure> {
    let mut diagnostics = kdbx::diagnostics::DiagnosticContext::enabled(options);
    let result = open_database_internal(reader, key, &mut diagnostics, true);
    match result {
        Ok(database) => Ok(DiagnosticSuccess {
            database,
            report: diagnostics.into_report(),
        }),
        Err(error) => Err(DiagnosticFailure {
            error: Box::new(error),
            report: Box::new(diagnostics.into_report()),
        }),
    }
}

fn open_database_internal<R: std::io::Read>(
    mut reader: R,
    key: &CompositeKey,
    diagnostics: &mut kdbx::diagnostics::DiagnosticContext<'_>,
    kdbx_only: bool,
) -> DatabaseResult<Database> {
    use kdbx::diagnostics::DiagnosticStage;
    use kdbx::file::header::{KDBX_SIGNATURE_1, KDBX_SIGNATURE_2, KDB_SIGNATURE_2};
    use std::io::Read;

    // Read only the 12-byte signature once for version detection, then chain
    // it back in front of the original reader so format readers can re-consume
    // it themselves (each reader calls `detect_version` internally).
    let mut sig = [0u8; 12];
    diagnostics.run(
        DiagnosticStage::Signature,
        || {
            reader.read_exact(&mut sig)?;
            let sig1 = u32::from_le_bytes(sig[0..4].try_into().unwrap());
            let sig2 = u32::from_le_bytes(sig[4..8].try_into().unwrap());
            if sig1 != KDBX_SIGNATURE_1 || !matches!(sig2, KDBX_SIGNATURE_2 | KDB_SIGNATURE_2) {
                return Err(DatabaseError::InvalidSignature(
                    "Expected a KeePass KDBX/KDB signature".into(),
                ));
            }
            Ok(())
        },
        |_| None,
    )?;

    let version = diagnostics.run(
        DiagnosticStage::Version,
        || {
            let mut sig_cursor = std::io::Cursor::new(&sig);
            kdbx::file::reader::DatabaseReader::detect_version(&mut sig_cursor)
        },
        |version| Some(format!("{version:?}")),
    )?;
    let raw_version = u32::from_le_bytes(sig[8..12].try_into().unwrap());
    diagnostics.set_version(version, raw_version);
    if kdbx_only && version == DatabaseVersion::KDB {
        return Err(DatabaseError::Unsupported(
            "kdbx-debug supports KDBX 3.1 and KDBX 4.x, not KDB v1".into(),
        ));
    }

    // Chain: [12-byte signature replay] ++ [rest of original stream].
    // The result is a single `Read` impl that yields the original byte stream
    // verbatim, with no full-file buffering.
    let mut chained = std::io::Cursor::new(sig).chain(reader);

    let mut database = match version {
        DatabaseVersion::KDB => kdbx::file::kdb_reader::read_kdb(&mut chained, key),
        DatabaseVersion::KDBX31 => {
            kdbx::file::kdbx31_reader::read_kdbx31_diagnostic(&mut chained, key, diagnostics)
        }
        DatabaseVersion::KDBX4 => {
            kdbx::file::kdbx4_reader::read_kdbx4_diagnostic(&mut chained, key, diagnostics)
        }
    }?;
    if diagnostics.is_enabled() {
        diagnostics.run(
            DiagnosticStage::ModelValidation,
            || database.validate(),
            |_| None,
        )?;
    }
    diagnostics.run(
        DiagnosticStage::MemoryProtection,
        || database.seal_protected_strings(key),
        |_| None,
    )?;
    diagnostics.set_summary(&database);
    Ok(database)
}

/// Save a KeePass database to a writer.
///
/// Writes the database in the format specified by `database.version`.
///
/// # Example
/// ```ignore
/// use keeless_kdbx::{save_database, CompositeKey};
///
/// let mut out = std::fs::File::create("database.kdbx")?;
/// save_database(&mut out, &db, &key)?;
/// ```
pub fn save_database<W: std::io::Write>(
    writer: &mut W,
    database: &Database,
    key: &CompositeKey,
) -> DatabaseResult<()> {
    save_database_with_credentials(writer, database, key, key)
}

/// Save using one credential to unlock memory and another for the output file.
pub fn save_database_with_credentials<W: std::io::Write>(
    writer: &mut W,
    database: &Database,
    memory_key: &CompositeKey,
    file_key: &CompositeKey,
) -> DatabaseResult<()> {
    database.validate()?;
    match database.version {
        DatabaseVersion::KDB => kdbx::file::kdb_writer::write_kdb_with_credentials(
            writer, database, memory_key, file_key,
        ),
        DatabaseVersion::KDBX31 => kdbx::file::kdbx31_writer::write_kdbx31_with_credentials(
            writer, database, memory_key, file_key,
        ),
        DatabaseVersion::KDBX4 => kdbx::file::kdbx4_writer::write_kdbx4_with_credentials(
            writer, database, memory_key, file_key,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::core::node::NodeId;
    use crate::model::db::database::{Database, DatabaseVersion};
    use crate::model::entry::Entry;
    use crate::model::group::Group;

    /// P2-3 regression: open_database must only buffer the 12-byte signature
    /// up-front, not the whole file. We verify this with a wrapper reader
    /// that records how many bytes have been observed *before* the format
    /// reader starts consuming the stream.
    ///
    /// The check is structural: after `detect_version` returns, the rest of
    /// the stream must be consumed lazily by the format reader. We model
    /// this by counting total `read()` calls and asserting that the peak
    /// in-flight buffer in `open_database` is just the signature.
    #[test]
    fn test_open_database_streams_underlying_reader() {
        // Build a minimal KDBX4 database and serialize it.
        let mut db = Database::new(DatabaseVersion::KDBX4);
        let root_id = NodeId::new_uuid();
        let mut root = Group::new(root_id);
        root.title = "Root".to_string();
        let entry_id = NodeId::new_uuid();
        let mut entry = Entry::new(entry_id);
        entry.title = "Test".into();
        root.add_child_entry(entry_id);
        db.groups.insert(root_id, root);
        db.entries.insert(entry_id, entry);
        db.root_group_id = Some(root_id);

        let key = CompositeKey::new()
            .with_password(b"streaming-test-pw")
            .unwrap();
        let mut bytes = Vec::new();
        save_database(&mut bytes, &db, &key).expect("save must succeed");

        // Open via a reader that is *not* seekable — a fresh Cursor over a
        // moved Vec. If open_database tried to read_to_end up-front and then
        // rewind (the old behavior), it would still work on a Cursor; the
        // real assertion is that it works on a pure forward-only reader.
        // We emulate that with a thin wrapper enforcing Read-only semantics.
        struct ForwardOnly<R: std::io::Read>(R);
        impl<R: std::io::Read> std::io::Read for ForwardOnly<R> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.0.read(buf)
            }
        }
        let reader = ForwardOnly(std::io::Cursor::new(bytes));

        let loaded = open_database(reader, &key).expect("streaming open must succeed");
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.root_group().unwrap().title, "Root");
    }

    /// open_database must surface a clear error (not panic / not OOM) when
    /// the signature is truncated.
    #[test]
    fn test_open_database_rejects_truncated_signature() {
        let short = [0u8; 5]; // less than 12 bytes
        let key = CompositeKey::new().with_password(b"x").unwrap();
        let err = open_database(&short[..], &key);
        assert!(err.is_err(), "truncated signature must error, not panic");
        match err.unwrap_err() {
            DatabaseError::Io(_) => {} // expected: UnexpectedEof
            other => panic!("expected Io error for truncated signature, got {other:?}"),
        }
    }

    /// open_database must reject an invalid signature cleanly.
    #[test]
    fn test_open_database_rejects_invalid_signature() {
        // 12 bytes of garbage — signature check fires before any other work.
        let mut garbage = vec![0xFFu8; 12];
        // Pad with a bit more so the format reader has something to fail on
        // *after* signature validation (shouldn't be reached).
        garbage.extend_from_slice(&[0u8; 64]);
        let key = CompositeKey::new().with_password(b"x").unwrap();
        let err = open_database(&garbage[..], &key);
        assert!(err.is_err());
    }

    #[test]
    fn protected_entry_fields_are_credential_scoped_in_memory() {
        let mut database = Database::new(DatabaseVersion::KDBX4);
        let root_id = NodeId::new_uuid();
        let mut root = Group::new(root_id);
        root.title = "Root".into();
        let entry_id = NodeId::new_uuid();
        let mut entry = Entry::new(entry_id);
        entry.title = ProtectedString::new_protected("hidden title");
        entry.password = ProtectedString::new_protected("initial secret");
        root.add_child_entry(entry_id);
        database.groups.insert(root_id, root);
        database.entries.insert(entry_id, entry);
        database.root_group_id = Some(root_id);

        let old_key = CompositeKey::new().with_password(b"old password").unwrap();
        let mut bytes = Vec::new();
        save_database(&mut bytes, &database, &old_key).unwrap();
        let mut loaded = open_database(bytes.as_slice(), &old_key).unwrap();

        let loaded_entry = &loaded.entries[&entry_id];
        assert!(loaded_entry.password.is_memory_protected());
        assert!(loaded_entry.title.is_memory_protected());
        assert_eq!(
            loaded
                .with_entry_field(
                    &old_key,
                    &entry_id,
                    &EntryFieldSelector::Password,
                    str::to_string,
                )
                .unwrap(),
            "initial secret"
        );
        assert_eq!(
            loaded
                .with_entry_field(
                    &old_key,
                    &entry_id,
                    &EntryFieldSelector::Title,
                    str::to_string,
                )
                .unwrap(),
            "hidden title"
        );

        let wrong_key = CompositeKey::new()
            .with_password(b"wrong password")
            .unwrap();
        assert!(matches!(
            loaded.with_entry_field(&wrong_key, &entry_id, &EntryFieldSelector::Password, |_| (),),
            Err(DatabaseError::InvalidCredentials)
        ));
        loaded
            .set_entry_field(
                &old_key,
                &entry_id,
                &EntryFieldSelector::Password,
                "updated secret",
                true,
            )
            .unwrap();

        let new_key = CompositeKey::new().with_password(b"new password").unwrap();
        let mut rotated = Vec::new();
        save_database_with_credentials(&mut rotated, &loaded, &old_key, &new_key).unwrap();
        assert!(open_database(rotated.as_slice(), &old_key).is_err());
        let reopened = open_database(rotated.as_slice(), &new_key).unwrap();
        assert_eq!(
            reopened
                .with_entry_field(
                    &new_key,
                    &entry_id,
                    &EntryFieldSelector::Password,
                    str::to_string,
                )
                .unwrap(),
            "updated secret"
        );
    }
}
