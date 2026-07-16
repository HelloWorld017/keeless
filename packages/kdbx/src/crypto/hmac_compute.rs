//! HMAC computation
//!
//! Provides HMAC-SHA-256 and HMAC-SHA-512.

use hmac::{Hmac, Mac};
use sha2::{Sha256, Sha512};

use super::{CryptoError, CryptoResult};

type HmacSha256 = Hmac<Sha256>;
type HmacSha512 = Hmac<Sha512>;

/// HMAC computation utilities.
pub struct HmacCompute;

impl HmacCompute {
    /// Compute HMAC-SHA-256.
    ///
    /// # Arguments
    /// * `key` - HMAC key
    /// * `data` - Input data
    pub fn hmac_sha256(key: &[u8], data: &[u8]) -> CryptoResult<[u8; 32]> {
        let mut mac =
            HmacSha256::new_from_slice(key).map_err(|e| CryptoError::HmacError(e.to_string()))?;
        mac.update(data);
        let result = mac.finalize().into_bytes();
        Ok(result.into())
    }

    /// Compute HMAC-SHA-512.
    pub fn hmac_sha512(key: &[u8], data: &[u8]) -> CryptoResult<[u8; 64]> {
        let mut mac =
            HmacSha512::new_from_slice(key).map_err(|e| CryptoError::HmacError(e.to_string()))?;
        mac.update(data);
        let result = mac.finalize().into_bytes();
        Ok(result.into())
    }

    /// Create a streaming HMAC-SHA-256 for incremental computation.
    pub fn hmac_sha256_stream(key: &[u8]) -> CryptoResult<HmacSha256> {
        HmacSha256::new_from_slice(key).map_err(|e| CryptoError::HmacError(e.to_string()))
    }

    /// Finalize a streaming HMAC-SHA-256.
    pub fn finalize_sha256(mac: HmacSha256) -> [u8; 32] {
        mac.finalize().into_bytes().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex;

    #[test]
    fn test_hmac_sha256_rfc4231() {
        // RFC 4231 Test Case 2
        let key = b"Jefe";
        let data = b"what do ya want for nothing?";
        let result = HmacCompute::hmac_sha256(key, data).unwrap();
        let expected =
            hex::decode("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843")
                .unwrap();
        assert_eq!(result.as_slice(), expected.as_slice());
    }

    #[test]
    fn test_hmac_sha256_streaming() {
        let key = b"testkey123";
        let one_shot = HmacCompute::hmac_sha256(key, b"helloworld").unwrap();

        let mut stream = HmacCompute::hmac_sha256_stream(key).unwrap();
        stream.update(b"hello");
        stream.update(b"world");
        let streamed = HmacCompute::finalize_sha256(stream);

        assert_eq!(one_shot, streamed);
    }
}
