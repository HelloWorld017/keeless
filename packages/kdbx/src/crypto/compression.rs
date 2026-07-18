//! Compression algorithm + utility functions
//!

use crate::kdbx::limits::MAX_DECOMPRESSED_PAYLOAD_SIZE;
use crate::model::exception::{DatabaseError, DatabaseResult};
use flate2::read::{GzDecoder, GzEncoder};
use flate2::Compression;
use serde::{Deserialize, Serialize};
use std::io::Read;
use zeroize::Zeroizing;

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
    decompress_with_limit(data, MAX_DECOMPRESSED_PAYLOAD_SIZE)
}

/// Decompress sensitive data into a buffer that is wiped on every exit path.
pub(crate) fn decompress_sensitive(data: &[u8]) -> DatabaseResult<Zeroizing<Vec<u8>>> {
    decompress_sensitive_with_limit(data, MAX_DECOMPRESSED_PAYLOAD_SIZE)
}

fn decompress_with_limit(data: &[u8], limit: usize) -> DatabaseResult<Vec<u8>> {
    let mut decoder = GzDecoder::new(data);
    let mut decompressed = Vec::new();
    read_decompressed(&mut decoder, &mut decompressed, limit)?;
    Ok(decompressed)
}

fn decompress_sensitive_with_limit(
    data: &[u8],
    limit: usize,
) -> DatabaseResult<Zeroizing<Vec<u8>>> {
    let mut decoder = GzDecoder::new(data);
    let mut decompressed = Zeroizing::new(Vec::new());
    read_decompressed(&mut decoder, &mut decompressed, limit)?;
    Ok(decompressed)
}

fn read_decompressed(
    decoder: &mut impl Read,
    decompressed: &mut Vec<u8>,
    limit: usize,
) -> DatabaseResult<()> {
    let mut chunk = Zeroizing::new([0u8; 16 * 1024]);
    loop {
        let read = decoder
            .read(chunk.as_mut_slice())
            .map_err(|e| DatabaseError::InvalidFormat(format!("Decompression error: {e}")))?;
        if read == 0 {
            break;
        }
        if decompressed
            .len()
            .checked_add(read)
            .is_none_or(|size| size > limit)
        {
            return Err(DatabaseError::InvalidFormat(format!(
                "Decompressed payload exceeds {limit} bytes"
            )));
        }
        decompressed.extend_from_slice(&chunk[..read]);
    }
    Ok(())
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
        assert_eq!(
            CompressionAlgorithm::from_id(0),
            Some(CompressionAlgorithm::None)
        );
        assert_eq!(
            CompressionAlgorithm::from_id(1),
            Some(CompressionAlgorithm::Gzip)
        );
        assert_eq!(CompressionAlgorithm::from_id(2), None);
    }

    #[test]
    fn test_decompression_limit_rejected() {
        let data = vec![0u8; 1025];
        let compressed = compress(&data).unwrap();
        assert!(matches!(
            decompress_with_limit(&compressed, 1024),
            Err(DatabaseError::InvalidFormat(_))
        ));
    }
}
