//! HMAC block stream for KDBX 4.0
//!
//! Provides read/write streaming with per-block HMAC-SHA-256 integrity verification.
//! This is the unified module used by both kdbx4_reader and kdbx4_writer.

use std::io::{Read, Write};

use crate::crypto::{HashEngine, HmacCompute};
use crate::kdbx::limits::{MAX_HMAC_BLOCK_SIZE, MAX_HMAC_PAYLOAD_SIZE};
use crate::model::exception::{DatabaseError, DatabaseResult};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

/// Default HMAC block size (1 MB)
pub const HMAC_BLOCK_SIZE: usize = 1024 * 1024;

// ─── Key derivation ──────────────────────────────────────────────

/// Derive the per-block HMAC key as SHA-512(index || base HMAC key).
pub fn derive_block_hmac_key(hmac_key: &[u8], block_index: u64) -> DatabaseResult<[u8; 64]> {
    Ok(HashEngine::sha512_multi(&[
        &block_index.to_le_bytes(),
        hmac_key,
    ]))
}

/// Compute HMAC-SHA-256 for a data block.
/// Input: block_index_le || block_size_le || data.
pub fn compute_block_hmac(key: &[u8], block_index: u64, data: &[u8]) -> DatabaseResult<[u8; 32]> {
    let size = u32::try_from(data.len())
        .map_err(|_| DatabaseError::InvalidFormat("HMAC block is too large".into()))?;
    let mut msg = Vec::with_capacity(12 + data.len());
    msg.extend_from_slice(&block_index.to_le_bytes());
    msg.extend_from_slice(&size.to_le_bytes());
    msg.extend_from_slice(data);
    HmacCompute::hmac_sha256(key, &msg).map_err(|e| DatabaseError::EncryptionError(e.to_string()))
}

/// Compute the header HMAC for KDBX 4.0 integrity check.
pub fn compute_header_hmac(hmac_key: &[u8], header_bytes: &[u8]) -> DatabaseResult<[u8; 32]> {
    let header_key = derive_block_hmac_key(hmac_key, u64::MAX)?;
    HmacCompute::hmac_sha256(&header_key, header_bytes)
        .map_err(|e| DatabaseError::EncryptionError(e.to_string()))
}

// ─── Stream Reader ───────────────────────────────────────────────

/// Read an HMAC block stream, verifying each block's integrity.
pub fn read_hmac_block_stream<R: Read>(reader: &mut R, hmac_key: &[u8]) -> DatabaseResult<Vec<u8>> {
    let mut result = Vec::new();
    let mut block_index: u64 = 0;

    loop {
        let mut stored_hmac = [0u8; 32];
        reader.read_exact(&mut stored_hmac)?;

        let block_size_u32 = reader.read_u32::<LittleEndian>()?;
        let block_size = usize::try_from(block_size_u32).map_err(|_| {
            DatabaseError::InvalidFormat("HMAC block size is not representable".into())
        })?;
        if block_size > MAX_HMAC_BLOCK_SIZE {
            return Err(DatabaseError::InvalidFormat(format!(
                "HMAC block exceeds {MAX_HMAC_BLOCK_SIZE} bytes"
            )));
        }
        let new_len = result
            .len()
            .checked_add(block_size)
            .ok_or_else(|| DatabaseError::InvalidFormat("HMAC payload size overflow".into()))?;
        if new_len > MAX_HMAC_PAYLOAD_SIZE {
            return Err(DatabaseError::InvalidFormat(format!(
                "HMAC payload exceeds {MAX_HMAC_PAYLOAD_SIZE} bytes"
            )));
        }

        let mut data = vec![0u8; block_size];
        reader.read_exact(&mut data)?;

        let block_key = derive_block_hmac_key(hmac_key, block_index)?;
        let expected = compute_block_hmac(&block_key, block_index, &data)?;
        if stored_hmac != expected {
            return Err(DatabaseError::DecryptionError(format!(
                "HMAC mismatch in block {block_index}"
            )));
        }

        if block_size == 0 {
            let mut trailing = [0u8; 1];
            loop {
                match reader.read(&mut trailing) {
                    Ok(0) => break,
                    Ok(_) => {
                        return Err(DatabaseError::InvalidFormat(
                            "Trailing data after HMAC terminator".into(),
                        ))
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e.into()),
                }
            }
            break;
        }

        result.extend_from_slice(&data);
        block_index += 1;
    }

    Ok(result)
}

// ─── Stream Writer ───────────────────────────────────────────────

/// Write data as an HMAC block stream.
pub fn write_hmac_block_stream<W: Write>(
    writer: &mut W,
    hmac_key: &[u8],
    data: &[u8],
) -> DatabaseResult<()> {
    if data.len() > MAX_HMAC_PAYLOAD_SIZE {
        return Err(DatabaseError::InvalidFormat(format!(
            "HMAC payload exceeds {MAX_HMAC_PAYLOAD_SIZE} bytes"
        )));
    }
    for (i, chunk) in data.chunks(HMAC_BLOCK_SIZE).enumerate() {
        let block_index = i as u64;
        let block_key = derive_block_hmac_key(hmac_key, block_index)?;
        let hmac_val = compute_block_hmac(&block_key, block_index, chunk)?;

        writer.write_all(&hmac_val)?;
        let chunk_len = u32::try_from(chunk.len())
            .map_err(|_| DatabaseError::InvalidFormat("HMAC block is too large".into()))?;
        writer.write_u32::<LittleEndian>(chunk_len)?;
        writer.write_all(chunk)?;
    }

    // Terminator block
    let term_index = (data.len().div_ceil(HMAC_BLOCK_SIZE)) as u64;
    let term_key = derive_block_hmac_key(hmac_key, term_index)?;
    let term_hmac = compute_block_hmac(&term_key, term_index, &[])?;
    writer.write_all(&term_hmac)?;
    writer.write_u32::<LittleEndian>(0)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_hmac_block_stream_roundtrip() {
        let data = vec![0x42u8; 2048];
        let key = b"master_key_for_test";

        let mut buf = Vec::new();
        write_hmac_block_stream(&mut buf, key, &data).unwrap();

        let mut cursor = Cursor::new(buf);
        let result = read_hmac_block_stream(&mut cursor, key).unwrap();

        assert_eq!(result, data);
    }

    #[test]
    fn test_hmac_block_stream_empty() {
        let data: Vec<u8> = Vec::new();
        let key = b"master_key";

        let mut buf = Vec::new();
        write_hmac_block_stream(&mut buf, key, &data).unwrap();

        let mut cursor = Cursor::new(buf);
        let result = read_hmac_block_stream(&mut cursor, key).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_hmac_block_stream_tamper_detected() {
        let data = vec![0x42u8; 1024];
        let key = b"master_key";

        let mut buf = Vec::new();
        write_hmac_block_stream(&mut buf, key, &data).unwrap();

        // Tamper with the data (after HMAC, before block data)
        // Layout: [32-byte hmac][4-byte size][data...]
        if buf.len() > 50 {
            buf[44] ^= 0xFF;
        }

        let mut cursor = Cursor::new(buf);
        assert!(read_hmac_block_stream(&mut cursor, key).is_err());
    }

    #[test]
    fn test_hmac_block_stream_wrong_key() {
        let data = vec![0x42u8; 512];
        let key1 = b"correct_key";
        let key2 = b"wrong_key";

        let mut buf = Vec::new();
        write_hmac_block_stream(&mut buf, key1, &data).unwrap();

        let mut cursor = Cursor::new(buf);
        assert!(read_hmac_block_stream(&mut cursor, key2).is_err());
    }

    #[test]
    fn test_derive_block_hmac_key_deterministic() {
        let key = b"test_key";
        let k1 = derive_block_hmac_key(key, 0).unwrap();
        let k2 = derive_block_hmac_key(key, 0).unwrap();
        assert_eq!(k1, k2);

        let k3 = derive_block_hmac_key(key, 1).unwrap();
        assert_ne!(k1, k3);
    }

    #[test]
    fn test_compute_header_hmac() {
        let master_key = b"master";
        let header = b"header_bytes";
        let h1 = compute_header_hmac(master_key, header).unwrap();
        let h2 = compute_header_hmac(master_key, header).unwrap();
        assert_eq!(h1, h2);

        // Different header → different HMAC
        let h3 = compute_header_hmac(master_key, b"different").unwrap();
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_hmac_block_stream_rejects_oversized_block_before_allocation() {
        let mut framed = vec![0u8; 32];
        framed.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            read_hmac_block_stream(&mut Cursor::new(framed), b"key"),
            Err(DatabaseError::InvalidFormat(_))
        ));
    }

    #[test]
    fn test_hmac_block_stream_rejects_trailing_data() {
        let mut framed = Vec::new();
        write_hmac_block_stream(&mut framed, b"key", b"payload").unwrap();
        framed.push(1);
        assert!(matches!(
            read_hmac_block_stream(&mut Cursor::new(framed), b"key"),
            Err(DatabaseError::InvalidFormat(_))
        ));
    }
}
