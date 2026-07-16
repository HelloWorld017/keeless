//! Digital signature verification for KDBX 4.0 databases
//!
//! KDBX 4.0 supports optional ECDSA and RSA public key signatures
//! for verifying database integrity beyond HMAC.

use p256::ecdsa::{VerifyingKey, Signature, signature::Verifier as EcdsaVerifier};
use rsa::pkcs1::DecodeRsaPublicKey;
use rsa::pkcs8::DecodePublicKey;

/// Signature algorithm type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureAlgorithm {
    /// ECDSA with NIST P-256 (secp256r1) curve — the KeePass default
    EcdsaP256,
    /// RSA with SHA-256
    RsaSha256,
    /// Legacy/unknown
    Unknown(u32),
}

impl SignatureAlgorithm {
    /// Get the algorithm ID used in KDBX headers.
    pub fn id(&self) -> u32 {
        match self {
            Self::EcdsaP256 => 0x0000_0001,
            Self::RsaSha256 => 0x0000_0002,
            Self::Unknown(id) => *id,
        }
    }

    /// Create from algorithm ID.
    pub fn from_id(id: u32) -> Self {
        match id {
            0x0000_0001 => Self::EcdsaP256,
            0x0000_0002 => Self::RsaSha256,
            _ => Self::Unknown(id),
        }
    }
}

/// A digital signature.
#[derive(Debug, Clone)]
pub struct DigitalSignature {
    /// The algorithm used
    pub algorithm: SignatureAlgorithm,
    /// The raw signature bytes (DER-encoded for ECDSA, PKCS#1 for RSA)
    pub signature: Vec<u8>,
}

impl DigitalSignature {
    pub fn new(algorithm: SignatureAlgorithm, signature: Vec<u8>) -> Self {
        Self { algorithm, signature }
    }
}

/// A public key for signature verification.
#[derive(Debug, Clone)]
pub struct PublicKey {
    /// The algorithm this key is for
    pub algorithm: SignatureAlgorithm,
    /// The raw public key bytes (SEC1 uncompressed for ECDSA, DER for RSA)
    pub key_data: Vec<u8>,
}

impl PublicKey {
    pub fn new(algorithm: SignatureAlgorithm, key_data: Vec<u8>) -> Self {
        Self { algorithm, key_data }
    }
}

/// Result of a signature verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureStatus {
    /// Signature is valid
    Valid,
    /// Signature is invalid
    Invalid,
    /// No signature present (most KDBX files don't have signatures)
    NotSigned,
    /// Public key not available for verification
    NoPublicKey,
    /// Unsupported signature algorithm
    UnsupportedAlgorithm(SignatureAlgorithm),
    /// Error during verification (malformed key or signature)
    VerificationError(String),
}

/// Signature verifier for KDBX databases.
pub struct SignatureVerifier;

impl SignatureVerifier {
    /// Verify a signature against data using the provided public key.
    pub fn verify(data: &[u8], signature: &DigitalSignature, public_key: &PublicKey) -> SignatureStatus {
        if signature.algorithm != public_key.algorithm {
            return SignatureStatus::UnsupportedAlgorithm(signature.algorithm);
        }

        match signature.algorithm {
            SignatureAlgorithm::EcdsaP256 => {
                Self::verify_ecdsa_p256(data, &signature.signature, &public_key.key_data)
            }
            SignatureAlgorithm::RsaSha256 => {
                Self::verify_rsa_sha256(data, &signature.signature, &public_key.key_data)
            }
            SignatureAlgorithm::Unknown(_) => {
                SignatureStatus::UnsupportedAlgorithm(signature.algorithm)
            }
        }
    }

    /// Quick check: is the signature algorithm supported?
    pub fn is_supported(algorithm: SignatureAlgorithm) -> bool {
        matches!(algorithm, SignatureAlgorithm::EcdsaP256 | SignatureAlgorithm::RsaSha256)
    }

    /// ECDSA P-256 verification using the `p256` crate.
    fn verify_ecdsa_p256(data: &[u8], signature_bytes: &[u8], public_key_bytes: &[u8]) -> SignatureStatus {
        // Parse the public key (SEC1 uncompressed format: 0x04 || x || y)
        let verifying_key = match VerifyingKey::from_sec1_bytes(public_key_bytes) {
            Ok(key) => key,
            Err(e) => return SignatureStatus::VerificationError(
                format!("Invalid ECDSA public key: {}", e)
            ),
        };

        // Parse the signature (DER-encoded)
        let signature = match Signature::from_der(signature_bytes) {
            Ok(sig) => sig,
            Err(e) => return SignatureStatus::VerificationError(
                format!("Invalid ECDSA signature: {}", e)
            ),
        };

        // Verify
        match verifying_key.verify(data, &signature) {
            Ok(()) => SignatureStatus::Valid,
            Err(_) => SignatureStatus::Invalid,
        }
    }

    /// RSA SHA-256 verification using the `rsa` crate.
    fn verify_rsa_sha256(data: &[u8], signature_bytes: &[u8], public_key_bytes: &[u8]) -> SignatureStatus {
        use rsa::signature::Verifier;
        use rsa::sha2::Sha256;
        use rsa::pkcs1v15::VerifyingKey as RsaVerifyingKey;
        use rsa::pkcs1v15::Signature as RsaSignature;

        // Parse RSA public key from DER PKCS#1, fallback to PKCS#8
        let public_key = match rsa::RsaPublicKey::from_pkcs1_der(public_key_bytes) {
            Ok(key) => key,
            Err(_) => {
                match rsa::RsaPublicKey::from_public_key_der(public_key_bytes) {
                    Ok(key) => key,
                    Err(e) => return SignatureStatus::VerificationError(
                        format!("Invalid RSA public key: {}", e)
                    ),
                }
            }
        };

        let verifying_key = RsaVerifyingKey::<Sha256>::new(public_key);

        let signature = match RsaSignature::try_from(signature_bytes) {
            Ok(sig) => sig,
            Err(e) => return SignatureStatus::VerificationError(
                format!("Invalid RSA signature: {}", e)
            ),
        };

        match verifying_key.verify(data, &signature) {
            Ok(()) => SignatureStatus::Valid,
            Err(_) => SignatureStatus::Invalid,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_algorithm_ids() {
        assert_eq!(SignatureAlgorithm::EcdsaP256.id(), 0x0000_0001);
        assert_eq!(SignatureAlgorithm::RsaSha256.id(), 0x0000_0002);
        assert_eq!(SignatureAlgorithm::Unknown(99).id(), 99);
    }

    #[test]
    fn test_signature_algorithm_from_id() {
        assert_eq!(SignatureAlgorithm::from_id(1), SignatureAlgorithm::EcdsaP256);
        assert_eq!(SignatureAlgorithm::from_id(2), SignatureAlgorithm::RsaSha256);
        assert_eq!(SignatureAlgorithm::from_id(42), SignatureAlgorithm::Unknown(42));
    }

    #[test]
    fn test_digital_signature_creation() {
        let sig = DigitalSignature::new(SignatureAlgorithm::EcdsaP256, vec![1, 2, 3]);
        assert_eq!(sig.algorithm, SignatureAlgorithm::EcdsaP256);
        assert_eq!(sig.signature, vec![1, 2, 3]);
    }

    #[test]
    fn test_public_key_creation() {
        let key = PublicKey::new(SignatureAlgorithm::RsaSha256, vec![4, 5, 6]);
        assert_eq!(key.algorithm, SignatureAlgorithm::RsaSha256);
        assert_eq!(key.key_data, vec![4, 5, 6]);
    }

    #[test]
    fn test_verify_algorithm_mismatch() {
        let sig = DigitalSignature::new(SignatureAlgorithm::EcdsaP256, vec![]);
        let key = PublicKey::new(SignatureAlgorithm::RsaSha256, vec![]);
        let status = SignatureVerifier::verify(&[1, 2, 3], &sig, &key);
        assert_eq!(status, SignatureStatus::UnsupportedAlgorithm(SignatureAlgorithm::EcdsaP256));
    }

    #[test]
    fn test_verify_unknown_algorithm() {
        let sig = DigitalSignature::new(SignatureAlgorithm::Unknown(99), vec![]);
        let key = PublicKey::new(SignatureAlgorithm::Unknown(99), vec![]);
        let status = SignatureVerifier::verify(&[], &sig, &key);
        assert_eq!(status, SignatureStatus::UnsupportedAlgorithm(SignatureAlgorithm::Unknown(99)));
    }

    #[test]
    fn test_is_supported() {
        assert!(SignatureVerifier::is_supported(SignatureAlgorithm::EcdsaP256));
        assert!(SignatureVerifier::is_supported(SignatureAlgorithm::RsaSha256));
        assert!(!SignatureVerifier::is_supported(SignatureAlgorithm::Unknown(99)));
    }

    #[test]
    fn test_ecdsa_invalid_key_returns_error() {
        let sig = DigitalSignature::new(
            SignatureAlgorithm::EcdsaP256,
            vec![0x30, 0x06, 0x02, 0x01, 0x01, 0x02, 0x01, 0x01],
        );
        let key = PublicKey::new(SignatureAlgorithm::EcdsaP256, vec![0x00]);
        let status = SignatureVerifier::verify(b"test data", &sig, &key);
        assert!(matches!(status, SignatureStatus::VerificationError(_)));
    }

    #[test]
    fn test_ecdsa_invalid_signature_returns_error() {
        // Valid P-256 public key (uncompressed point, 65 bytes — generator point G)
        let valid_pubkey = vec![
            0x04,
            0x6B, 0x17, 0xD1, 0xF2, 0xE1, 0x2C, 0x42, 0x47, 0x48, 0x8B, 0xA2, 0x8E, 0x97, 0x14, 0x8E, 0x2F,
            0x5F, 0x09, 0x75, 0x53, 0x9F, 0x3D, 0x58, 0x09, 0x93, 0xE4, 0x83, 0x7B, 0x5D, 0x4D, 0xC2, 0x6B,
            0x4F, 0xE1, 0x8F, 0xD4, 0x77, 0x10, 0x53, 0x3E, 0x24, 0x89, 0x7B, 0x41, 0xC6, 0x3E, 0x72, 0x18,
            0x3E, 0xE9, 0x78, 0x7B, 0x2E, 0x47, 0xA4, 0x28, 0xA0, 0x8A, 0x83, 0xE4, 0x3E, 0xA6, 0x83, 0x9D,
        ];
        let sig = DigitalSignature::new(SignatureAlgorithm::EcdsaP256, vec![0x00]);
        let key = PublicKey::new(SignatureAlgorithm::EcdsaP256, valid_pubkey);
        let status = SignatureVerifier::verify(b"test data", &sig, &key);
        assert!(matches!(status, SignatureStatus::VerificationError(_)));
    }
}
