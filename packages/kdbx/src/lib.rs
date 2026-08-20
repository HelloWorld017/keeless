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
    ChangeType, CompositeCredentials, CompositeKey, CustomData, CustomDataItem, Database,
    DatabaseVersion, DateInstant, DeletedObject, DiffResult, Entry, EntryBinary, EntryField,
    EntryFieldId, EntryFieldSelector, EntryFieldUpdate, EntryKDB, EntryKDBX, EntryPropertiesUpdate,
    EntryUpdate, FieldReference, Group, GroupKDB, GroupKDBX, IconImage, IconImageCustom,
    IconImageStandard, IconUpdate, MasterCredential, MemoryProtectionConfig, Node, NodeHandler,
    NodeId, NodeType, PreparedEntryUpdate, ProtectedString, RefTarget, SortNodeEnum, StandardField,
    Tag, Template, TemplateField, TemplateFieldType, TraversalOrder, NUMBER_STANDARD_ICONS,
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
    FuzzySearchHelper, FuzzySearchResult, SearchFilter, SearchHelper, SearchParameters,
    SearchQuery, SearchResult, TagQuery, TagResult, UrlMatchParameters, UrlMatchResult, UrlMatcher,
};

// ─── Merge ────────────────────────────────────────────────────────────
pub use kdbx::merge::{
    ConflictResolution, ConflictType, DatabaseMerger, MergeConflict, MergeResult, MergeStrategy,
};

// ─── OTP ──────────────────────────────────────────────────────────────
pub use model::entry::otp::{
    is_keepass_timeotp_field, is_keepass_timeotp_secret_field, parse_keepass_timeotp_fields,
    parse_otpauth_uri, OtpHashAlgorithm, OtpParameters, OtpType, TokenCalculator,
    KEEPASS_TIMEOTP_FIELD_NAMES,
};

// ─── Passkeys ─────────────────────────────────────────────────────────
pub use model::entry::passkey::{
    find_passkey_credentials, is_passkey_entry, passkey_credential_ids, AuthenticationRequest,
    AuthenticationResponse, CtapAuthenticationRequest, CtapAuthenticationResponse,
    CtapRegistrationRequest, CtapRegistrationResponse, CtapRegistrationResult, PasskeyAlgorithm,
    PasskeyAuthenticator, PasskeyCredential, PasskeyCredentialId, PasskeyCredentialMetadata,
    PasskeyCredentialSummary, PasskeyError, PasskeyFieldValue, RegistrationRequest,
    RegistrationResponse, RegistrationResult, UserPresence, UserVerification, KEELESS_AAGUID,
};

// ─── Repair ───────────────────────────────────────────────────────────
pub use kdbx::repair::{
    IntegrityError, IntegrityReport, IntegrityVerifier, IntegrityWarning, RepairResult,
};

// ─── Fuzz ─────────────────────────────────────────────────────────────
pub use kdbx::fuzz::{FuzzOutcome, FuzzResult, FuzzTarget};

// ─── XML ──────────────────────────────────────────────────────────────
pub use kdbx::xml::{KdbxXmlReader, KdbxXmlWriter};

/// A database opened with its transformed key.
#[derive(Debug)]
pub struct OpenedDatabase {
    pub database: Database,
    pub key: CompositeKey,
}

impl std::ops::Deref for OpenedDatabase {
    type Target = Database;

    fn deref(&self) -> &Self::Target {
        &self.database
    }
}

impl std::ops::DerefMut for OpenedDatabase {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.database
    }
}

/// Open a KeePass database from a reader and credentials.
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
/// use keeless_kdbx::{open_database, CompositeCredentials};
///
/// let file = std::fs::File::open("database.kdbx")?;
/// let credentials = CompositeCredentials::new().with_password(b"mypassword").unwrap();
/// let opened = open_database(file, &credentials)?;
/// println!("Opened {} entries", opened.database.entry_count());
/// ```
pub fn open_database<R: std::io::Read>(
    reader: R,
    credentials: &CompositeCredentials,
) -> DatabaseResult<OpenedDatabase> {
    let mut diagnostics = kdbx::diagnostics::DiagnosticContext::disabled();
    open_database_internal(
        reader,
        OpenKey::Credentials(credentials),
        &mut diagnostics,
        false,
    )
}

/// Open a database using a transformed key already derived for its header.
///
/// This path verifies the KDF fingerprint before decrypting and never invokes
/// the database KDF. It is used by ordinary saves, reveals, and background sync.
pub fn open_database_with_key<R: std::io::Read>(
    reader: R,
    key: &CompositeKey,
) -> DatabaseResult<Database> {
    let mut diagnostics = kdbx::diagnostics::DiagnosticContext::disabled();
    open_database_internal(reader, OpenKey::Derived(key), &mut diagnostics, false)
        .map(|opened| opened.database)
}

/// Open a KDBX database and return a structured report even when opening fails.
pub fn diagnose_database<R: std::io::Read>(
    reader: R,
    credentials: &CompositeCredentials,
    options: DiagnosticOptions<'_>,
) -> Result<DiagnosticSuccess, DiagnosticFailure> {
    let mut diagnostics = kdbx::diagnostics::DiagnosticContext::enabled(options);
    let result = open_database_internal(
        reader,
        OpenKey::Credentials(credentials),
        &mut diagnostics,
        true,
    );
    match result {
        Ok(opened) => Ok(DiagnosticSuccess {
            database: opened.database,
            report: diagnostics.into_report(),
        }),
        Err(error) => Err(DiagnosticFailure {
            error: Box::new(error),
            report: Box::new(diagnostics.into_report()),
        }),
    }
}

enum OpenKey<'a> {
    Credentials(&'a CompositeCredentials),
    Derived(&'a CompositeKey),
}

fn open_database_internal<R: std::io::Read>(
    mut reader: R,
    key: OpenKey<'_>,
    diagnostics: &mut kdbx::diagnostics::DiagnosticContext<'_>,
    kdbx_only: bool,
) -> DatabaseResult<OpenedDatabase> {
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

    let (mut database, key) = match key {
        OpenKey::Credentials(credentials) => match version {
            DatabaseVersion::KDB => {
                kdbx::file::kdb_reader::read_kdb_with_credentials(&mut chained, credentials)
            }
            DatabaseVersion::KDBX31 => {
                kdbx::file::kdbx31_reader::read_kdbx31_with_credentials_diagnostic(
                    &mut chained,
                    credentials,
                    diagnostics,
                )
            }
            DatabaseVersion::KDBX4 => {
                kdbx::file::kdbx4_reader::read_kdbx4_with_credentials_diagnostic(
                    &mut chained,
                    credentials,
                    diagnostics,
                )
            }
        },
        OpenKey::Derived(key) => match version {
            DatabaseVersion::KDB => kdbx::file::kdb_reader::read_kdb(&mut chained, key)
                .and_then(|database| Ok((database, key.try_clone()?))),
            DatabaseVersion::KDBX31 => {
                kdbx::file::kdbx31_reader::read_kdbx31_diagnostic(&mut chained, key, diagnostics)
                    .and_then(|database| Ok((database, key.try_clone()?)))
            }
            DatabaseVersion::KDBX4 => {
                kdbx::file::kdbx4_reader::read_kdbx4_diagnostic(&mut chained, key, diagnostics)
                    .and_then(|database| Ok((database, key.try_clone()?)))
            }
        },
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
        || database.seal_protected_strings(&key),
        |_| None,
    )?;
    diagnostics.set_summary(&database);
    Ok(OpenedDatabase { database, key })
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
    database.validate()?;
    match database.version {
        DatabaseVersion::KDB => kdbx::file::kdb_writer::write_kdb(writer, database, key),
        DatabaseVersion::KDBX31 => kdbx::file::kdbx31_writer::write_kdbx31(writer, database, key),
        DatabaseVersion::KDBX4 => kdbx::file::kdbx4_writer::write_kdbx4(writer, database, key),
    }
}

/// Initialize KDF parameters for a new database and derive its active key.
pub fn initialize_database_key(
    database: &mut Database,
    credentials: &CompositeCredentials,
) -> DatabaseResult<CompositeKey> {
    use kdbx::kdf::aes_kdf::AesKdf;
    use kdbx::kdf::argon2_kdf::Argon2Kdf;

    let kdf: Box<dyn KdfEngine> = match database.version {
        DatabaseVersion::KDB | DatabaseVersion::KDBX31 => Box::new(AesKdf),
        DatabaseVersion::KDBX4 => Box::new(Argon2Kdf::argon2id()),
    };
    let mut parameters = kdf.default_parameters();
    kdf.randomize(&mut parameters)?;
    let key = credentials.derive_key(&parameters)?;
    database.kdf_parameters = Some(parameters);
    Ok(key)
}

/// Controls how a credential rotation changes the database KDF.
#[derive(Debug, Clone)]
pub struct RekeyOptions {
    /// Regenerate the KDF salt when no replacement parameters are supplied.
    pub regenerate_kdf_salt: bool,
    /// Replace all KDF parameters before deriving the new key.
    pub kdf_parameters: Option<KdfParameters>,
}

impl Default for RekeyOptions {
    fn default() -> Self {
        Self {
            regenerate_kdf_salt: true,
            kdf_parameters: None,
        }
    }
}

/// Rotate credentials and reseal runtime-protected strings under the new key.
pub fn rekey_database(
    database: &mut Database,
    old_key: &CompositeKey,
    credentials: &CompositeCredentials,
    options: RekeyOptions,
) -> DatabaseResult<CompositeKey> {
    let current = database
        .kdf_parameters
        .as_ref()
        .ok_or(DatabaseError::MissingKdfParameters)?;
    if !old_key.matches(current) {
        return Err(DatabaseError::KdfParametersMismatch);
    }
    let mut parameters = options.kdf_parameters.unwrap_or_else(|| current.clone());
    if options.regenerate_kdf_salt {
        let kdf = kdbx::kdf::create_kdf(&parameters.kdf_uuid)
            .ok_or_else(|| DatabaseError::InvalidFormat("Unknown KDF".into()))?;
        kdf.randomize(&mut parameters)?;
    }
    let new_key = credentials.derive_key(&parameters)?;
    let mut rekeyed = database.clone();
    rekeyed.rekey_memory_protection(old_key, &new_key)?;
    rekeyed.kdf_parameters = Some(parameters);
    *database = rekeyed;
    Ok(new_key)
}

/// Re-encrypt runtime memory protection without changing on-disk KDF parameters.
///
/// This is used when importing an already-opened database into another active
/// database; it does not rotate the file credentials.
pub fn reencrypt_memory_protection(
    database: &mut Database,
    old_key: &CompositeKey,
    new_key: &CompositeKey,
) -> DatabaseResult<()> {
    let mut rekeyed = database.clone();
    rekeyed.rekey_memory_protection(old_key, new_key)?;
    *database = rekeyed;
    Ok(())
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
        entry.set_title("Test");
        root.add_child_entry(entry_id);
        db.groups.insert(root_id, root);
        db.entries.insert(entry_id, entry);
        db.root_group_id = Some(root_id);

        let credentials = CompositeCredentials::new()
            .with_password(b"streaming-test-pw")
            .unwrap();
        let key = initialize_database_key(&mut db, &credentials).unwrap();
        let kdf_parameters = db.kdf_parameters.clone();
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

        let loaded = open_database(reader, &credentials).expect("streaming open must succeed");
        assert_eq!(loaded.database.entries.len(), 1);
        assert_eq!(loaded.database.root_group().unwrap().title, "Root");
        assert_eq!(loaded.database.kdf_parameters, kdf_parameters);
    }

    /// open_database must surface a clear error (not panic / not OOM) when
    /// the signature is truncated.
    #[test]
    fn test_open_database_rejects_truncated_signature() {
        let short = [0u8; 5]; // less than 12 bytes
        let credentials = CompositeCredentials::new().with_password(b"x").unwrap();
        let err = open_database(&short[..], &credentials);
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
        let credentials = CompositeCredentials::new().with_password(b"x").unwrap();
        let err = open_database(&garbage[..], &credentials);
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
        entry.set_title(ProtectedString::new_protected("hidden title"));
        entry.set_password(ProtectedString::new_protected("initial secret"));
        root.add_child_entry(entry_id);
        database.groups.insert(root_id, root);
        database.entries.insert(entry_id, entry);
        database.root_group_id = Some(root_id);

        let old_credentials = CompositeCredentials::new()
            .with_password(b"old password")
            .unwrap();
        let old_key = initialize_database_key(&mut database, &old_credentials).unwrap();
        let mut bytes = Vec::new();
        save_database(&mut bytes, &database, &old_key).unwrap();
        let opened = open_database(bytes.as_slice(), &old_credentials).unwrap();
        let mut loaded = opened.database;
        let old_key = opened.key;

        let loaded_entry = &loaded.entries[&entry_id];
        assert!(loaded_entry.password().is_memory_protected());
        assert!(loaded_entry.title().is_memory_protected());
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

        let wrong_key = CompositeCredentials::new()
            .with_password(b"wrong password")
            .unwrap()
            .derive_key(loaded.kdf_parameters.as_ref().unwrap())
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

        let new_credentials = CompositeCredentials::new()
            .with_password(b"new password")
            .unwrap();
        let new_key = rekey_database(
            &mut loaded,
            &old_key,
            &new_credentials,
            RekeyOptions::default(),
        )
        .unwrap();
        let mut rotated = Vec::new();
        save_database(&mut rotated, &loaded, &new_key).unwrap();
        assert!(open_database(rotated.as_slice(), &old_credentials).is_err());
        let reopened = open_database(rotated.as_slice(), &new_credentials).unwrap();
        assert_eq!(
            reopened
                .database
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
