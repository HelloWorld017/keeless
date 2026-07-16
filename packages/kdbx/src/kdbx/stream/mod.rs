//! Stream processing utilities
//!

pub mod copy_stream;
pub mod hashed_block;
pub mod hmac_block_stream;

pub use hashed_block::{HashedBlockReader, HashedBlockWriter};
pub use hmac_block_stream::{
    read_hmac_block_stream, write_hmac_block_stream,
    compute_header_hmac, compute_block_hmac, derive_block_hmac_key,
    HMAC_BLOCK_SIZE,
};
