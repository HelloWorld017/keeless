//! Database root structure
//!

mod validation;

mod entry;
pub use entry::EntryFieldSelector;

mod entry_update;
pub use entry_update::{EntryFieldUpdate, EntryPropertiesUpdate, IconUpdate, PreparedEntryUpdate};

mod group;
mod recycle_bin;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::Arc;

use keeless_secure_types::SecureArray;

use crate::crypto::compression::CompressionAlgorithm;
use crate::crypto::encryption_algorithm::EncryptionAlgorithm;
use crate::crypto::memory_protection::{MemoryProtectionContext, MemoryUnlockSession};
use crate::kdbx::file::header::{FILE_VERSION_31, FILE_VERSION_4};
use crate::kdbx::kdf::argon2_kdf::Argon2Kdf;
use crate::kdbx::kdf::kdf_engine::KdfEngine;
use crate::kdbx::kdf::kdf_parameters::KdfParameters;
use crate::model::core::node::NodeId;
use crate::model::core::security::MemoryProtectionConfig;
use crate::model::db::composite_key::CompositeKey;
use crate::model::entry::Entry;
use crate::model::exception::{DatabaseError, DatabaseResult};
use crate::model::group::Group;
use crate::model::meta::icon::IconImageCustom;
use crate::model::meta::{CustomData, DeletedObject};
use crate::model::xml::DatabaseXmlExtensions;
use uuid::Uuid;
/// Database version
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseVersion {
    /// KDB format (KeePass 1.x)
    KDB,
    /// KDBX 3.1 (KeePass 2.x pre-4)
    KDBX31,
    /// KDBX 4.0 (KeePass 2.x post-4)
    KDBX4,
}

/// The main KeePass database structure.
#[derive(Debug, Clone)]
pub struct Database {
    /// Database version
    pub version: DatabaseVersion,
    /// Exact on-disk format version, including the KDBX minor version.
    pub file_version: u32,
    /// Root group ID.
    ///
    /// **Single source of truth**: the root group is always looked up in
    /// `self.groups` via [`root_group`]`/`[`root_group_mut`]. We deliberately
    /// do NOT cache the root `Group` value here — a stale cached copy was the
    /// root cause of "added entries/groups disappear after save & reopen",
    /// because mutators updated `self.groups` but readers/writers read the
    /// stale cache.
    pub root_group_id: Option<NodeId>,
    /// All groups indexed by ID
    pub groups: HashMap<NodeId, Group>,
    /// All entries indexed by ID
    pub entries: HashMap<NodeId, Entry>,
    /// Deleted objects (KDBX 4.0 recycle bin)
    pub deleted_objects: Vec<DeletedObject>,
    /// Custom icons
    pub custom_icons: HashMap<Uuid, IconImageCustom>,
    /// Encryption algorithm
    pub encryption_algorithm: EncryptionAlgorithm,
    /// Compression algorithm
    pub compression: CompressionAlgorithm,
    /// KDF parameters
    pub kdf_parameters: Option<KdfParameters>,
    /// Raw KDBX4 public custom-data variant dictionary.
    pub public_custom_data: Vec<u8>,
    /// Optional KDBX4 outer-header comment.
    pub header_comment: Option<Vec<u8>>,
    /// Master key hash for verification
    pub master_key_hash: Option<Vec<u8>>,
    /// Database name
    pub name: String,
    /// Database description
    pub description: String,
    /// Default username
    pub default_username: String,
    /// Is the database loaded?
    pub loaded: bool,
    /// Is read-only mode enabled?
    pub is_read_only: bool,
    /// Data modified since last save?
    pub data_modified: bool,
    /// Recycle bin group UUID
    pub recycle_bin_uuid: Option<Uuid>,
    /// Entry templates group UUID
    pub entry_templates_uuid: Option<Uuid>,
    /// Default protection settings stored in Meta/MemoryProtection.
    pub memory_protection: MemoryProtectionConfig,
    /// Extensible database-level custom data.
    pub custom_data: CustomData,
    /// Set when parsing encountered XML understood only as an opaque extension.
    pub contains_unsupported_xml: bool,
    /// Opaque XML elements retained for forward-compatible round-trips.
    #[doc(hidden)]
    pub xml_extensions: DatabaseXmlExtensions,
    pub(crate) memory_protection_context: Option<Arc<MemoryProtectionContext>>,
}

impl Database {
    pub fn new(version: DatabaseVersion) -> Self {
        let file_version = match version {
            DatabaseVersion::KDB => 0x0001_0003,
            DatabaseVersion::KDBX31 => FILE_VERSION_31,
            DatabaseVersion::KDBX4 => FILE_VERSION_4,
        };
        Self {
            version,
            file_version,
            root_group_id: None,
            groups: HashMap::new(),
            entries: HashMap::new(),
            deleted_objects: Vec::new(),
            custom_icons: HashMap::new(),
            encryption_algorithm: EncryptionAlgorithm::AesRijndael,
            compression: CompressionAlgorithm::Gzip,
            kdf_parameters: None,
            public_custom_data: Vec::new(),
            header_comment: None,
            master_key_hash: None,
            name: String::new(),
            description: String::new(),
            default_username: String::new(),
            loaded: false,
            is_read_only: false,
            data_modified: false,
            recycle_bin_uuid: None,
            entry_templates_uuid: None,
            memory_protection: MemoryProtectionConfig {
                protect_password: true,
                ..MemoryProtectionConfig::default()
            },
            custom_data: CustomData::default(),
            contains_unsupported_xml: false,
            xml_extensions: DatabaseXmlExtensions::default(),
            memory_protection_context: None,
        }
    }

    /// Encrypt all KDBX-protected entry strings before exposing a loaded database.
    pub(crate) fn seal_protected_strings(
        &mut self,
        composite_key: &CompositeKey,
    ) -> DatabaseResult<()> {
        let context = match &self.memory_protection_context {
            Some(context) => {
                let context = context.clone();
                let mut unlock = MemoryUnlockSession::new(composite_key);
                unlock.with_root(&context, |root| {
                    for entry in self.entries.values_mut() {
                        entry.seal_protected_strings(context.clone(), root)?;
                    }
                    Ok(())
                })?;
                context
            }
            None => {
                let (context, root) = self.create_memory_context(composite_key)?;
                root.unlock(|root| {
                    for entry in self.entries.values_mut() {
                        entry.seal_protected_strings(context.clone(), root)?;
                    }
                    Ok::<_, DatabaseError>(())
                })??;
                context
            }
        };
        self.memory_protection_context = Some(context);
        Ok(())
    }

    /// Seal protected strings added through low-level model APIs.
    pub fn protect_entry_strings(&mut self, composite_key: &CompositeKey) -> DatabaseResult<()> {
        self.seal_protected_strings(composite_key)
    }

    pub(crate) fn memory_unlock<'a>(
        &self,
        composite_key: &'a CompositeKey,
    ) -> MemoryUnlockSession<'a> {
        MemoryUnlockSession::new(composite_key)
    }

    fn create_memory_context(
        &self,
        composite_key: &CompositeKey,
    ) -> DatabaseResult<(Arc<MemoryProtectionContext>, SecureArray<32>)> {
        let parameters = self.kdf_parameters.clone().unwrap_or_else(|| {
            let kdf = Argon2Kdf::argon2id();
            kdf.default_parameters()
        });
        MemoryProtectionContext::create(composite_key, parameters)
    }

    /// Mark the database as modified.
    pub fn mark_modified(&mut self) {
        self.data_modified = true;
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new(DatabaseVersion::KDBX4)
    }
}
