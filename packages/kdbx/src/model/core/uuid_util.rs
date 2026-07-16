//! UUID utilities
//!

use uuid::Uuid;

/// UUID utility functions
pub struct UuidUtil;

impl UuidUtil {
    /// Convert 16 bytes to a UUID
    pub fn from_bytes(bytes: &[u8; 16]) -> Uuid {
        Uuid::from_bytes(*bytes)
    }

    /// Convert a UUID to 16 bytes
    pub fn to_bytes(uuid: &Uuid) -> [u8; 16] {
        *uuid.as_bytes()
    }

    /// Generate a new random UUID
    pub fn new_random() -> Uuid {
        Uuid::new_v4()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_bytes() {
        let bytes: [u8; 16] = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
            0x32, 0x10,
        ];
        let uuid = UuidUtil::from_bytes(&bytes);
        assert_eq!(uuid.as_bytes(), &bytes);
    }

    #[test]
    fn test_to_bytes() {
        let uuid = Uuid::parse_str("01234567-89ab-cdef-fedc-ba9876543210").unwrap();
        let bytes = UuidUtil::to_bytes(&uuid);
        assert_eq!(
            bytes,
            [
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
                0x32, 0x10
            ]
        );
    }

    #[test]
    fn test_new_random_generates_different_uuids() {
        let a = UuidUtil::new_random();
        let b = UuidUtil::new_random();
        assert_ne!(a, b, "two random UUIDs should differ");
    }

    #[test]
    fn test_roundtrip_from_bytes_to_bytes() {
        let original: [u8; 16] = [
            0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC,
            0xDE, 0xF0,
        ];
        let uuid = UuidUtil::from_bytes(&original);
        let roundtripped = UuidUtil::to_bytes(&uuid);
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn test_nil_uuid() {
        let nil = Uuid::nil();
        let bytes = UuidUtil::to_bytes(&nil);
        assert_eq!(bytes, [0u8; 16]);

        let from_nil_bytes = UuidUtil::from_bytes(&[0u8; 16]);
        assert_eq!(from_nil_bytes, Uuid::nil());
    }
}
