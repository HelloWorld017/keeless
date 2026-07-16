//! ChaCha20 stream cipher
//!
//! Uses ChaCha20-Poly1305 (IETF variant, RFC 8439 / ChaCha7539)

use chacha20::cipher::{KeyIvInit, StreamCipher as StreamCipherTrait};
use chacha20::ChaCha20;

use super::{CryptoError, CryptoResult};

const CHACHA20_KEY_SIZE: usize = 32;
const CHACHA20_IV_SIZE: usize = 12; // IETF variant uses 96-bit nonce

/// ChaCha20 cipher for KDBX 4.0 inner stream encryption.
pub struct ChaCha20Cipher {
    cipher: ChaCha20,
}

impl ChaCha20Cipher {
    /// Create a new ChaCha20 cipher.
    ///
    /// # Arguments
    /// * `key` - 32-byte key
    /// * `iv` - 12-byte nonce (IETF variant)
    pub fn new(key: &[u8], iv: &[u8]) -> CryptoResult<Self> {
        if key.len() != CHACHA20_KEY_SIZE {
            return Err(CryptoError::InvalidKeyLength {
                expected: CHACHA20_KEY_SIZE,
                got: key.len(),
            });
        }
        if iv.len() != CHACHA20_IV_SIZE {
            return Err(CryptoError::InvalidIvLength {
                expected: CHACHA20_IV_SIZE,
                got: iv.len(),
            });
        }

        let cipher = ChaCha20::new_from_slices(key, iv)
            .map_err(|e| CryptoError::EncryptionFailed(e.to_string()))?;

        Ok(Self { cipher })
    }

    /// Process data (encrypt/decrypt - same operation for stream ciphers).
    pub fn process(&mut self, data: &[u8]) -> Vec<u8> {
        let mut buf = data.to_vec();
        self.cipher.apply_keystream(&mut buf);
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chacha20_encrypt_decrypt() {
        let key = [0x42u8; 32];
        let iv = [0x13u8; 12];
        let plaintext = b"Hello ChaCha20!";

        let mut enc = ChaCha20Cipher::new(&key, &iv).unwrap();
        let ciphertext = enc.process(plaintext);

        let mut dec = ChaCha20Cipher::new(&key, &iv).unwrap();
        let decrypted = dec.process(&ciphertext);

        assert_eq!(decrypted, plaintext);
    }
}
