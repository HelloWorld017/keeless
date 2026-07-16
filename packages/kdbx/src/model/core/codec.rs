//! Codec utilities
//!

use byteorder::{BigEndian, ByteOrder, LittleEndian};

/// Codec utilities for byte manipulation
pub struct CodecUtil;

impl CodecUtil {
    /// Read a u16 from bytes (big-endian)
    pub fn read_u16_be(data: &[u8]) -> u16 {
        BigEndian::read_u16(data)
    }

    /// Read a u16 from bytes (little-endian)
    pub fn read_u16_le(data: &[u8]) -> u16 {
        LittleEndian::read_u16(data)
    }

    /// Read a u32 from bytes (little-endian)
    pub fn read_u32_le(data: &[u8]) -> u32 {
        LittleEndian::read_u32(data)
    }

    /// Write a u32 to bytes (little-endian)
    pub fn write_u32_le(value: u32) -> [u8; 4] {
        let mut buf = [0u8; 4];
        LittleEndian::write_u32(&mut buf, value);
        buf
    }

    /// Read a u64 from bytes (little-endian)
    pub fn read_u64_le(data: &[u8]) -> u64 {
        LittleEndian::read_u64(data)
    }

    /// Write a u64 to bytes (little-endian)
    pub fn write_u64_le(value: u64) -> [u8; 8] {
        let mut buf = [0u8; 8];
        LittleEndian::write_u64(&mut buf, value);
        buf
    }

    /// XOR two byte arrays (modifies first in place)
    pub fn xor_in_place(a: &mut [u8], b: &[u8]) {
        for (ai, bi) in a.iter_mut().zip(b.iter()) {
            *ai ^= bi;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CodecUtil;

    // ── read_u16_be ──────────────────────────────────────────────

    #[test]
    fn test_read_u16_be_zero() {
        assert_eq!(CodecUtil::read_u16_be(&[0x00, 0x00]), 0x0000u16);
    }

    #[test]
    fn test_read_u16_be_max() {
        assert_eq!(CodecUtil::read_u16_be(&[0xFF, 0xFF]), 0xFFFFu16);
    }

    #[test]
    fn test_read_u16_be_typical() {
        // 0x12 0x34 → 0x1234
        assert_eq!(CodecUtil::read_u16_be(&[0x12, 0x34]), 0x1234u16);
    }

    #[test]
    fn test_read_u16_be_high_byte_only() {
        assert_eq!(CodecUtil::read_u16_be(&[0x01, 0x00]), 0x0100u16);
    }

    #[test]
    fn test_read_u16_be_low_byte_only() {
        assert_eq!(CodecUtil::read_u16_be(&[0x00, 0x01]), 0x0001u16);
    }

    // ── read_u16_le ──────────────────────────────────────────────

    #[test]
    fn test_read_u16_le_zero() {
        assert_eq!(CodecUtil::read_u16_le(&[0x00, 0x00]), 0x0000u16);
    }

    #[test]
    fn test_read_u16_le_max() {
        assert_eq!(CodecUtil::read_u16_le(&[0xFF, 0xFF]), 0xFFFFu16);
    }

    #[test]
    fn test_read_u16_le_typical() {
        // LE: 0x34 0x12 → 0x1234
        assert_eq!(CodecUtil::read_u16_le(&[0x34, 0x12]), 0x1234u16);
    }

    #[test]
    fn test_read_u16_le_low_byte_only() {
        assert_eq!(CodecUtil::read_u16_le(&[0x01, 0x00]), 0x0001u16);
    }

    #[test]
    fn test_read_u16_le_high_byte_only() {
        assert_eq!(CodecUtil::read_u16_le(&[0x00, 0x01]), 0x0100u16);
    }

    // ── read_u32_le ──────────────────────────────────────────────

    #[test]
    fn test_read_u32_le_zero() {
        assert_eq!(
            CodecUtil::read_u32_le(&[0x00, 0x00, 0x00, 0x00]),
            0x00000000u32
        );
    }

    #[test]
    fn test_read_u32_le_max() {
        assert_eq!(
            CodecUtil::read_u32_le(&[0xFF, 0xFF, 0xFF, 0xFF]),
            0xFFFFFFFFu32
        );
    }

    #[test]
    fn test_read_u32_le_typical() {
        // LE: 0xCD 0xAB 0x00 0x00 → 0x0000ABCD
        assert_eq!(
            CodecUtil::read_u32_le(&[0xCD, 0xAB, 0x00, 0x00]),
            0x0000ABCDu32
        );
    }

    #[test]
    fn test_read_u32_le_one() {
        assert_eq!(CodecUtil::read_u32_le(&[0x01, 0x00, 0x00, 0x00]), 1u32);
    }

    // ── write_u32_le ─────────────────────────────────────────────

    #[test]
    fn test_write_u32_le_zero() {
        assert_eq!(CodecUtil::write_u32_le(0u32), [0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_write_u32_le_max() {
        assert_eq!(
            CodecUtil::write_u32_le(0xFFFFFFFFu32),
            [0xFF, 0xFF, 0xFF, 0xFF]
        );
    }

    #[test]
    fn test_write_u32_le_typical() {
        assert_eq!(
            CodecUtil::write_u32_le(0x0000ABCDu32),
            [0xCD, 0xAB, 0x00, 0x00]
        );
    }

    #[test]
    fn test_write_u32_le_one() {
        assert_eq!(CodecUtil::write_u32_le(1u32), [0x01, 0x00, 0x00, 0x00]);
    }

    // ── round-trip u32 ───────────────────────────────────────────

    #[test]
    fn test_roundtrip_u32_le() {
        let values = [0u32, 1, 0xFF, 0xFFFF, 0x12345678, 0xFFFFFFFF];
        for v in values {
            let bytes = CodecUtil::write_u32_le(v);
            assert_eq!(CodecUtil::read_u32_le(&bytes), v);
        }
    }

    // ── read_u64_le ──────────────────────────────────────────────

    #[test]
    fn test_read_u64_le_zero() {
        assert_eq!(
            CodecUtil::read_u64_le(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
            0u64
        );
    }

    #[test]
    fn test_read_u64_le_max() {
        assert_eq!(
            CodecUtil::read_u64_le(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
            0xFFFFFFFFFFFFFFFFu64
        );
    }

    #[test]
    fn test_read_u64_le_typical() {
        // LE: 0x78 0x56 0x34 0x12 0x00 0x00 0x00 0x00 → 0x0000000012345678
        assert_eq!(
            CodecUtil::read_u64_le(&[0x78, 0x56, 0x34, 0x12, 0x00, 0x00, 0x00, 0x00]),
            0x0000000012345678u64
        );
    }

    #[test]
    fn test_read_u64_le_one() {
        assert_eq!(
            CodecUtil::read_u64_le(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
            1u64
        );
    }

    // ── write_u64_le ─────────────────────────────────────────────

    #[test]
    fn test_write_u64_le_zero() {
        assert_eq!(
            CodecUtil::write_u64_le(0u64),
            [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn test_write_u64_le_max() {
        assert_eq!(
            CodecUtil::write_u64_le(0xFFFFFFFFFFFFFFFFu64),
            [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
        );
    }

    #[test]
    fn test_write_u64_le_typical() {
        assert_eq!(
            CodecUtil::write_u64_le(0x0000000012345678u64),
            [0x78, 0x56, 0x34, 0x12, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn test_write_u64_le_one() {
        assert_eq!(
            CodecUtil::write_u64_le(1u64),
            [0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
    }

    // ── round-trip u64 ───────────────────────────────────────────

    #[test]
    fn test_roundtrip_u64_le() {
        let values = [
            0u64,
            1,
            0xFF,
            0xFFFF,
            0x123456789ABCDEF0,
            0xFFFFFFFFFFFFFFFF,
        ];
        for v in values {
            let bytes = CodecUtil::write_u64_le(v);
            assert_eq!(CodecUtil::read_u64_le(&bytes), v);
        }
    }

    // ── xor_in_place ─────────────────────────────────────────────

    #[test]
    fn test_xor_in_place_zeros() {
        let mut a = [0x00u8, 0x00, 0x00, 0x00];
        let b = [0x00u8, 0x00, 0x00, 0x00];
        CodecUtil::xor_in_place(&mut a, &b);
        assert_eq!(a, [0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_xor_in_place_max() {
        let mut a = [0xFFu8, 0xFF, 0xFF];
        let b = [0xFFu8, 0xFF, 0xFF];
        CodecUtil::xor_in_place(&mut a, &b);
        assert_eq!(a, [0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_xor_in_place_self() {
        let mut a = [0xABu8, 0xCD, 0xEF];
        let b = [0xABu8, 0xCD, 0xEF];
        CodecUtil::xor_in_place(&mut a, &b);
        assert_eq!(a, [0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_xor_in_place_typical() {
        let mut a = [0x0Fu8, 0xF0, 0xAA, 0x55];
        let b = [0xF0u8, 0x0F, 0x55, 0xAA];
        CodecUtil::xor_in_place(&mut a, &b);
        assert_eq!(a, [0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_xor_in_place_asymmetric_lengths() {
        // b is shorter — only the overlapping portion should be XORed
        let mut a = [0x01u8, 0x02, 0x03, 0x04];
        let b = [0xFFu8, 0xFF];
        CodecUtil::xor_in_place(&mut a, &b);
        assert_eq!(a, [0xFE, 0xFD, 0x03, 0x04]);
    }

    #[test]
    fn test_xor_in_place_empty() {
        let mut a: [u8; 0] = [];
        let b: [u8; 0] = [];
        CodecUtil::xor_in_place(&mut a, &b);
        assert_eq!(a, [] as [u8; 0]);
    }

    #[test]
    fn test_xor_in_place_single_byte() {
        let mut a = [0x42u8];
        let b = [0xFFu8];
        CodecUtil::xor_in_place(&mut a, &b);
        assert_eq!(a, [0xBD]);
    }

    #[test]
    fn test_xor_in_place_double_xor_restores() {
        let original = [0xDEu8, 0xAD, 0xBE, 0xEF];
        let key = [0x11u8, 0x22, 0x33, 0x44];
        let mut a = original;
        CodecUtil::xor_in_place(&mut a, &key);
        assert_ne!(a, original);
        CodecUtil::xor_in_place(&mut a, &key);
        assert_eq!(a, original);
    }
}
