//! Binary data handling: binary pool, cache, and stream I/O.

//!

use serde::{Deserialize, Serialize};

/// A single binary attachment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryData {
    /// Unique identifier within the pool
    pub id: u32,
    /// Raw binary data
    pub data: Vec<u8>,
    /// Whether the data is gzipped (KDBX 4.0 flag)
    pub is_compressed: bool,
    /// Whether the data is protected (KDBX 4.0 flag)
    pub is_protected: bool,
}

impl BinaryData {
    /// Create a new binary attachment
    pub fn new(id: u32, data: Vec<u8>) -> Self {
        Self {
            id,
            data,
            is_compressed: false,
            is_protected: false,
        }
    }

    /// Create with compression flag
    pub fn new_compressed(id: u32, data: Vec<u8>) -> Self {
        Self {
            id,
            data,
            is_compressed: true,
            is_protected: false,
        }
    }
}

/// Pool of binary data objects
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BinaryPool {
    pub binaries: Vec<BinaryData>,
}

impl BinaryPool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a binary, returning its index/id
    pub fn add(&mut self, data: Vec<u8>) -> u32 {
        let id = self.binaries.len() as u32;
        self.binaries.push(BinaryData::new(id, data));
        id
    }

    /// Add a compressed binary
    pub fn add_compressed(&mut self, data: Vec<u8>) -> u32 {
        let id = self.binaries.len() as u32;
        self.binaries.push(BinaryData::new_compressed(id, data));
        id
    }

    /// Get a binary by id
    pub fn get(&self, id: u32) -> Option<&BinaryData> {
        self.binaries.iter().find(|b| b.id == id)
    }

    /// Get a mutable reference to a binary by id
    pub fn get_mut(&mut self, id: u32) -> Option<&mut BinaryData> {
        self.binaries.iter_mut().find(|b| b.id == id)
    }

    /// Remove a binary by id
    pub fn remove(&mut self, id: u32) -> Option<BinaryData> {
        if let Some(pos) = self.binaries.iter().position(|b| b.id == id) {
            Some(self.binaries.remove(pos))
        } else {
            None
        }
    }

    /// Number of binaries
    pub fn len(&self) -> usize {
        self.binaries.len()
    }

    /// Is the pool empty?
    pub fn is_empty(&self) -> bool {
        self.binaries.is_empty()
    }
}

/// Cache for binary data that's cleared when database is locked
#[derive(Debug, Clone, Default)]
pub struct BinaryCache {
    data: Vec<Vec<u8>>,
}

impl BinaryCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.data.clear();
    }
}

// Binary streaming for large attachments
// loading everything into memory at once.

use std::fs::File;
use std::io::{self, Read, Write};

/// A streaming reader for binary attachments.
/// Reads binary data in chunks rather than loading everything at once.
pub struct BinaryStreamReader<'a> {
    pool: &'a BinaryPool,
    current_id: Option<u32>,
    position: usize,
    #[allow(dead_code)]
    chunk_size: usize,
}

impl<'a> BinaryStreamReader<'a> {
    pub fn new(pool: &'a BinaryPool, binary_id: u32) -> Self {
        Self {
            pool,
            current_id: Some(binary_id),
            position: 0,
            chunk_size: 8192,
        }
    }

    /// Set the chunk size for reading.
    pub fn with_chunk_size(mut self, size: usize) -> Self {
        self.chunk_size = size.max(1);
        self
    }

    /// Get the total length of the binary data.
    pub fn len(&self) -> Option<usize> {
        self.current_id
            .and_then(|id| self.pool.get(id))
            .map(|b| b.data.len())
    }

    /// Check if there's no data.
    pub fn is_empty(&self) -> bool {
        self.len().is_none_or(|l| l == 0)
    }
}

impl<'a> Read for BinaryStreamReader<'a> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let id = match self.current_id {
            Some(id) => id,
            None => return Ok(0),
        };

        let binary = match self.pool.get(id) {
            Some(b) => b,
            None => return Err(io::Error::new(io::ErrorKind::NotFound, "Binary not found")),
        };

        if self.position >= binary.data.len() {
            return Ok(0);
        }

        let remaining = &binary.data[self.position..];
        let to_read = buf.len().min(remaining.len());
        buf[..to_read].copy_from_slice(&remaining[..to_read]);
        self.position += to_read;
        Ok(to_read)
    }
}

/// A streaming writer for building binary attachments.
/// Writes data to a buffer that can be added to a BinaryPool.
pub struct BinaryStreamWriter {
    buffer: Vec<u8>,
    #[allow(dead_code)]
    chunk_size: usize,
}

impl Default for BinaryStreamWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl BinaryStreamWriter {
    /// Create a new writer with default chunk size.
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            chunk_size: 8192,
        }
    }

    /// Create a writer with a pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            chunk_size: 8192,
        }
    }

    /// Finalize the writer, adding the data to the pool.
    /// Returns the binary ID.
    pub fn finish(self, pool: &mut BinaryPool) -> u32 {
        pool.add(self.buffer)
    }

    /// Finalize as a compressed binary.
    pub fn finish_compressed(self, pool: &mut BinaryPool) -> u32 {
        pool.add_compressed(self.buffer)
    }

    /// Get the current buffer length.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if nothing has been written yet.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

impl Write for BinaryStreamWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Export a binary attachment to a file on disk.
pub fn export_binary_to_file(pool: &BinaryPool, binary_id: u32, path: &str) -> io::Result<u64> {
    let binary = pool
        .get(binary_id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Binary not found"))?;

    let mut file = File::create(path)?;
    let data = if binary.is_compressed {
        // Decompress on the fly
        let mut decoder = flate2::read::DeflateDecoder::new(&binary.data[..]);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        decompressed
    } else {
        binary.data.clone()
    };

    file.write_all(&data)?;
    Ok(data.len() as u64)
}

/// Import a file from disk into a binary pool.
pub fn import_file_to_pool(path: &str, pool: &mut BinaryPool, compress: bool) -> io::Result<u32> {
    let mut file = File::open(path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;

    if compress {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&data)?;
        let compressed = encoder.finish()?;
        Ok(pool.add_compressed(compressed))
    } else {
        Ok(pool.add(data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_data_creation() {
        let bd = BinaryData::new(0, vec![1, 2, 3]);
        assert_eq!(bd.id, 0);
        assert!(!bd.is_compressed);
        assert!(!bd.is_protected);
    }

    #[test]
    fn test_binary_pool_add_get() {
        let mut pool = BinaryPool::new();
        let id0 = pool.add(vec![1, 2, 3]);
        let id1 = pool.add(vec![4, 5, 6]);
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(pool.get(0).unwrap().data, vec![1, 2, 3]);
        assert_eq!(pool.get(1).unwrap().data, vec![4, 5, 6]);
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn test_binary_pool_remove() {
        let mut pool = BinaryPool::new();
        pool.add(vec![1, 2, 3]);
        pool.add(vec![4, 5, 6]);
        let removed = pool.remove(0).unwrap();
        assert_eq!(removed.data, vec![1, 2, 3]);
        assert_eq!(pool.len(), 1);
        assert!(pool.get(0).is_none());
    }
}

#[test]
fn test_stream_reader() {
    let mut pool = BinaryPool::new();
    let id = pool.add(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

    let mut reader = BinaryStreamReader::new(&pool, id).with_chunk_size(3);
    let mut result = Vec::new();
    reader.read_to_end(&mut result).unwrap();

    assert_eq!(result, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
}

#[test]
fn test_stream_reader_chunked() {
    let mut pool = BinaryPool::new();
    let id = pool.add(vec![1, 2, 3, 4, 5]);

    let mut reader = BinaryStreamReader::new(&pool, id).with_chunk_size(2);
    let mut buf = [0u8; 2];

    let n = reader.read(&mut buf).unwrap();
    assert_eq!(n, 2);
    assert_eq!(buf, [1, 2]);

    let n = reader.read(&mut buf).unwrap();
    assert_eq!(n, 2);
    assert_eq!(buf, [3, 4]);

    let n = reader.read(&mut buf).unwrap();
    assert_eq!(n, 1);
    assert_eq!(buf[0], 5);

    let n = reader.read(&mut buf).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn test_stream_reader_not_found() {
    let pool = BinaryPool::new();
    let mut reader = BinaryStreamReader::new(&pool, 999);
    let mut buf = [0u8; 10];
    let result = reader.read(&mut buf);
    assert!(result.is_err());
}

#[test]
fn test_stream_writer() {
    let mut writer = BinaryStreamWriter::new();
    writer.write_all(b"hello").unwrap();
    writer.write_all(b" world").unwrap();

    let mut pool = BinaryPool::new();
    let id = writer.finish(&mut pool);

    assert_eq!(pool.get(id).unwrap().data, b"hello world");
}

#[test]
fn test_stream_writer_with_capacity() {
    let writer = BinaryStreamWriter::with_capacity(1024);
    assert!(writer.is_empty());
    assert_eq!(writer.len(), 0);
}

#[test]
fn test_stream_writer_compressed() {
    let mut writer = BinaryStreamWriter::new();
    writer.write_all(b"compressed data").unwrap();

    let mut pool = BinaryPool::new();
    let id = writer.finish_compressed(&mut pool);

    assert!(pool.get(id).unwrap().is_compressed);
}

#[test]
fn test_stream_reader_empty() {
    let mut pool = BinaryPool::new();
    let id = pool.add(vec![]);

    let reader = BinaryStreamReader::new(&pool, id);
    assert!(reader.is_empty());
}

#[test]
fn test_stream_reader_len() {
    let mut pool = BinaryPool::new();
    let id = pool.add(vec![1, 2, 3]);

    let reader = BinaryStreamReader::new(&pool, id);
    assert_eq!(reader.len(), Some(3));
}
