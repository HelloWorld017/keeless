//! KDBX file header definitions
//!

use crate::crypto::compression::CompressionAlgorithm;
use crate::crypto::encryption_algorithm::EncryptionAlgorithm;
use crate::kdbx::kdf::kdf_parameters::KdfParameters;

/// KDBX file signature constants
pub const KDBX_SIGNATURE_1: u32 = 0x9AA2D903;
pub const KDBX_SIGNATURE_2: u32 = 0xB54BFB67;
pub const KDB_SIGNATURE_1: u32 = 0x9AA2D903;
pub const KDB_SIGNATURE_2: u32 = 0xB54BFB66;

/// File version constants
pub const FILE_VERSION_31: u32 = 0x00030001;
pub const FILE_VERSION_4: u32 = 0x00040000;
pub const FILE_VERSION_40: u32 = 0x00040000;

/// KDBX 3.1 outer header field IDs
pub mod header_field_31 {
    pub const END_OF_HEADER: u8 = 0;
    pub const COMMENT: u8 = 1;
    pub const CIPHER_ID: u8 = 2;
    pub const COMPRESSION_FLAGS: u8 = 3;
    pub const MASTER_SEED: u8 = 4;
    pub const TRANSFORM_SEED: u8 = 5;
    pub const TRANSFORM_ROUNDS: u8 = 6;
    pub const ENCRYPTION_IV: u8 = 7;
    pub const INNER_RANDOM_STREAM_KEY: u8 = 8;
    pub const STREAM_START_BYTES: u8 = 9;
    pub const INNER_RANDOM_STREAM_ID: u8 = 10;
}

/// KDBX 4.0 outer header field IDs
pub mod header_field_4 {
    pub const END_OF_HEADER: u8 = 0;
    pub const COMMENT: u8 = 1;
    pub const CIPHER_ID: u8 = 2;
    pub const COMPRESSION_FLAGS: u8 = 3;
    pub const MASTER_SEED: u8 = 4;
    pub const ENCRYPTION_IV: u8 = 7;
    pub const KDF_PARAMETERS: u8 = 11;
    pub const PUBLIC_CUSTOM_DATA: u8 = 12;
}

/// KDBX 4.0 inner header field IDs
pub mod inner_header_field_4 {
    pub const END_OF_HEADER: u8 = 0;
    pub const INNER_RANDOM_STREAM_ID: u8 = 1;
    pub const INNER_RANDOM_STREAM_KEY: u8 = 2;
    pub const BINARY: u8 = 3;
}

/// CRS (inner stream cipher) algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrsAlgorithm {
    None = 0,
    ArcFourVariant = 1,
    Salsa20 = 2,
    ChaCha20 = 3,
}

impl CrsAlgorithm {
    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            0 => Some(CrsAlgorithm::None),
            1 => Some(CrsAlgorithm::ArcFourVariant),
            2 => Some(CrsAlgorithm::Salsa20),
            3 => Some(CrsAlgorithm::ChaCha20),
            _ => None,
        }
    }

    pub fn to_id(&self) -> u32 {
        *self as u32
    }
}

/// KDBX 3.1 outer header.
#[derive(Debug, Clone)]
pub struct KdbxHeader31 {
    pub version: u32,
    pub encryption_algorithm: EncryptionAlgorithm,
    pub compression: CompressionAlgorithm,
    pub master_seed: Vec<u8>,
    pub transform_seed: Vec<u8>,
    pub transform_rounds: u64,
    pub encryption_iv: Vec<u8>,
    pub inner_random_stream_key: Vec<u8>,
    pub stream_start_bytes: Vec<u8>,
    pub inner_random_stream: CrsAlgorithm,
}

/// KDBX 4.0 outer header.
#[derive(Debug, Clone)]
pub struct KdbxHeader4 {
    pub version: u32,
    pub comment: Option<Vec<u8>>,
    pub encryption_algorithm: EncryptionAlgorithm,
    pub compression: CompressionAlgorithm,
    pub master_seed: Vec<u8>,
    pub encryption_iv: Vec<u8>,
    pub kdf_parameters: Option<KdfParameters>,
    pub public_custom_data: Vec<u8>,
}

/// KDBX 4.0 inner header.
#[derive(Debug, Clone)]
pub struct KdbxInnerHeader4 {
    pub inner_random_stream: CrsAlgorithm,
    pub inner_random_stream_key: Vec<u8>,
    pub binaries: Vec<KdbxBinary>,
}

/// Binary entry in KDBX 4.0 inner header.
#[derive(Debug, Clone)]
pub struct KdbxBinary {
    pub flags: u8,
    pub data: Vec<u8>,
}

impl KdbxBinary {
    pub fn is_protected(&self) -> bool {
        (self.flags & 0x01) != 0
    }
}

/// KDB header.
#[derive(Debug, Clone)]
pub struct KdbHeader {
    pub flags: u32,
    pub version: u32,
    pub master_seed: Vec<u8>,
    pub encryption_iv: Vec<u8>,
    pub number_of_groups: u32,
    pub number_of_entries: u32,
    pub content_hash: [u8; 32],
    pub transform_seed: Vec<u8>,
    pub transform_rounds: u32,
}

/// Unified header representation.
#[derive(Debug, Clone)]
pub enum DatabaseHeader {
    Kdb(KdbHeader),
    Kdbx31(KdbxHeader31),
    Kdbx4(KdbxHeader4, KdbxInnerHeader4),
}

impl DatabaseHeader {
    /// Get the master seed.
    pub fn master_seed(&self) -> &[u8] {
        match self {
            DatabaseHeader::Kdb(h) => &h.master_seed,
            DatabaseHeader::Kdbx31(h) => &h.master_seed,
            DatabaseHeader::Kdbx4(h, _) => &h.master_seed,
        }
    }

    /// Get the encryption IV.
    pub fn encryption_iv(&self) -> &[u8] {
        match self {
            DatabaseHeader::Kdb(h) => &h.encryption_iv,
            DatabaseHeader::Kdbx31(h) => &h.encryption_iv,
            DatabaseHeader::Kdbx4(h, _) => &h.encryption_iv,
        }
    }

    /// Get the encryption algorithm.
    pub fn encryption_algorithm(&self) -> EncryptionAlgorithm {
        match self {
            DatabaseHeader::Kdb(_) => EncryptionAlgorithm::AesRijndael,
            DatabaseHeader::Kdbx31(h) => h.encryption_algorithm,
            DatabaseHeader::Kdbx4(h, _) => h.encryption_algorithm,
        }
    }
}
