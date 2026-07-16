//! Conservative resource limits for parsing untrusted database files.

pub(crate) const MAX_OUTER_HEADER_FIELD_SIZE: usize = 1024 * 1024;
pub(crate) const MAX_OUTER_HEADER_SIZE: usize = 4 * 1024 * 1024;
pub(crate) const MAX_INNER_HEADER_FIELD_SIZE: usize = 64 * 1024 * 1024;
pub(crate) const MAX_INNER_HEADER_SIZE: usize = 256 * 1024 * 1024;
pub(crate) const MAX_HMAC_BLOCK_SIZE: usize = 16 * 1024 * 1024;
pub(crate) const MAX_HMAC_PAYLOAD_SIZE: usize = 512 * 1024 * 1024;
pub(crate) const MAX_DECOMPRESSED_PAYLOAD_SIZE: usize = 512 * 1024 * 1024;
pub(crate) const MAX_XML_NESTING_DEPTH: usize = 128;

pub(crate) const MAX_ARGON2_MEMORY_BYTES: u64 = 1024 * 1024 * 1024;
pub(crate) const MAX_ARGON2_ITERATIONS: u64 = 10_000;
pub(crate) const MAX_ARGON2_PARALLELISM: u32 = 64;
pub(crate) const MAX_AES_KDF_ROUNDS: u64 = 100_000_000;
