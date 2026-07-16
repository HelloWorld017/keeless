//! Hashed block stream (KDBX 3.1 integrity verification)
//!

use crate::crypto::HashEngine;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

use crate::model::exception::{DatabaseError, DatabaseResult};

const HASHED_BLOCK_SIZE: usize = 1024 * 1024; // 1 MB

/// Hashed block reader for KDBX 3.1
#[allow(dead_code)]
pub struct HashedBlockReader<R: Read> {
    inner: R,
    buffer: Vec<u8>,
    position: usize,
    done: bool,
}

impl<R: Read> HashedBlockReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            buffer: Vec::new(),
            position: 0,
            done: false,
        }
    }

    /// Read all verified blocks into a buffer.
    pub fn read_all(&mut self) -> DatabaseResult<Vec<u8>> {
        let mut result = Vec::new();

        while let Ok(block_index) = self.inner.read_u32::<LittleEndian>() {
            let stored_hash = {
                let mut hash = [0u8; 32];
                self.inner.read_exact(&mut hash)?;
                hash
            };

            let data_size = self.inner.read_u32::<LittleEndian>()? as usize;
            if data_size == 0 {
                break;
            }

            let mut data = vec![0u8; data_size];
            self.inner.read_exact(&mut data)?;

            // Verify hash
            let computed = HashEngine::sha256(&data);
            if computed != stored_hash {
                return Err(DatabaseError::DecryptionError(format!(
                    "Hash mismatch in block {block_index}"
                )));
            }

            result.extend_from_slice(&data);
        }

        Ok(result)
    }
}

/// Hashed block writer for KDBX 3.1
pub struct HashedBlockWriter<W: Write> {
    inner: W,
    block_index: u32,
}

impl<W: Write> HashedBlockWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            block_index: 0,
        }
    }

    /// Write data as hashed blocks.
    pub fn write_all(&mut self, data: &[u8]) -> DatabaseResult<()> {
        for chunk in data.chunks(HASHED_BLOCK_SIZE) {
            self.inner.write_u32::<LittleEndian>(self.block_index)?;

            let hash = HashEngine::sha256(chunk);
            self.inner.write_all(&hash)?;

            self.inner.write_u32::<LittleEndian>(chunk.len() as u32)?;
            self.inner.write_all(chunk)?;

            self.block_index += 1;
        }

        // Write terminator block
        self.inner.write_u32::<LittleEndian>(self.block_index)?;
        self.inner.write_all(&[0u8; 32])?; // Zero hash
        self.inner.write_u32::<LittleEndian>(0)?; // Zero size

        Ok(())
    }

    /// Consume the writer and return the inner writer
    pub fn into_inner(self) -> W {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_hashed_block_roundtrip() {
        let data = vec![0x42u8; 2048];

        let mut buf = Vec::new();
        {
            let mut writer = HashedBlockWriter::new(&mut buf);
            writer.write_all(&data).unwrap();
        }

        let mut reader = HashedBlockReader::new(Cursor::new(buf));
        let result = reader.read_all().unwrap();

        assert_eq!(result, data);
    }

    #[test]
    fn test_hashed_block_empty() {
        let data: Vec<u8> = Vec::new();

        let mut buf = Vec::new();
        {
            let mut writer = HashedBlockWriter::new(&mut buf);
            writer.write_all(&data).unwrap();
        }

        let mut reader = HashedBlockReader::new(Cursor::new(buf));
        let result = reader.read_all().unwrap();
        assert!(result.is_empty());
    }
}
