//! Stream processing utilities
//!

pub mod copy_stream;
pub mod hashed_block;
pub mod hmac_block_stream;

pub use hashed_block::{HashedBlockReader, HashedBlockWriter};
pub use hmac_block_stream::{
    compute_block_hmac, compute_header_hmac, derive_block_hmac_key, read_hmac_block_stream,
    write_hmac_block_stream, HMAC_BLOCK_SIZE,
};
