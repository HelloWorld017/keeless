//! Compression algorithm + utility functions
//!

use std::io::Read;
use flate2::read::{GzDecoder, GzEncoder};
use flate2::Compression;
use serde::{Deserialize, Serialize};
use crate::model::exception::{DatabaseError, DatabaseResult};

/// Compression algorithm used in KDBX files
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    #[default]
    None,
    Gzip,
}

impl CompressionAlgorithm {
    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            0 => Some(CompressionAlgorithm::None),
            1 => Some(CompressionAlgorithm::Gzip),
            _ => None,
        }
    }

    pub fn to_id(&self) -> u32 {
        match self {
            CompressionAlgorithm::None => 0,
            CompressionAlgorithm::Gzip => 1,
        }
    }
}

/// Compress data with gzip.
pub fn compress(data: &[u8]) -> DatabaseResult<Vec<u8>> {
    let mut encoder = GzEncoder::new(data, Compression::default());
    let mut compressed = Vec::new();
    encoder
        .read_to_end(&mut compressed)
        .map_err(|e| DatabaseError::InvalidFormat(format!("Compression error: {e}")))?;
    Ok(compressed)
}

/// Decompress gzip data.
pub fn decompress(data: &[u8]) -> DatabaseResult<Vec<u8>> {
    let mut decoder = GzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| DatabaseError::InvalidFormat(format!("Decompression error: {e}")))?;
    Ok(decompressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_decompress_roundtrip() {
        let data = b"Hello, KeePass! This is a test of gzip compression.";
        let compressed = compress(data).unwrap();
        assert_ne!(compressed.as_slice(), data.as_slice());
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_compression_algorithm_id() {
        assert_eq!(CompressionAlgorithm::None.to_id(), 0);
        assert_eq!(CompressionAlgorithm::Gzip.to_id(), 1);
        assert_eq!(CompressionAlgorithm::from_id(0), Some(CompressionAlgorithm::None));
        assert_eq!(CompressionAlgorithm::from_id(1), Some(CompressionAlgorithm::Gzip));
        assert_eq!(CompressionAlgorithm::from_id(2), None);
    }
}
