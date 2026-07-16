//! Copy input stream utility
//!

use std::io::{Read, Write};

/// Copy all data from a reader to a writer.
pub fn copy_stream<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> std::io::Result<u64> {
    std::io::copy(reader, writer)
}
