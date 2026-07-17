//! Salsa20 stream cipher
//!
//! Used for KDBX 3.1 inner stream protection.

use salsa20::cipher::{KeyIvInit, StreamCipher as StreamCipherTrait};
use salsa20::Salsa20;

use super::hash::HashEngine;
use super::{CryptoError, CryptoResult};
use zeroize::Zeroizing;

const SALSA20_KEY_SIZE: usize = 32;
const SALSA20_IV_SIZE: usize = 8; // Salsa20 uses 64-bit nonce

/// Hardcoded Salsa20 IV used in KeePass.
const KEEPASS_SALSA_IV: [u8; 8] = [0xE8, 0x30, 0x09, 0x4B, 0x97, 0x20, 0x5D, 0x2A];

/// Salsa20 stream cipher for KDBX 3.1 protected fields.
pub struct Salsa20Cipher {
    cipher: Salsa20,
}

impl Salsa20Cipher {
    /// Create a new Salsa20 cipher with the KeePass convention.
    /// The key is first SHA-256 hashed, then used with the hardcoded IV.
    ///
    pub fn new(key: &[u8]) -> Self {
        // SHA-256 hash the key first (KeePass convention)
        let key32 = Zeroizing::new(HashEngine::sha256(key));

        let cipher = Salsa20::new_from_slices(key32.as_slice(), &KEEPASS_SALSA_IV)
            .expect("Salsa20 key/iv sizes are correct");

        Self { cipher }
    }

    /// Create a Salsa20 cipher with explicit key and IV (raw mode).
    pub fn new_raw(key: &[u8], iv: &[u8]) -> CryptoResult<Self> {
        if key.len() != SALSA20_KEY_SIZE {
            return Err(CryptoError::InvalidKeyLength {
                expected: SALSA20_KEY_SIZE,
                got: key.len(),
            });
        }
        if iv.len() != SALSA20_IV_SIZE {
            return Err(CryptoError::InvalidIvLength {
                expected: SALSA20_IV_SIZE,
                got: iv.len(),
            });
        }

        let cipher = Salsa20::new_from_slices(key, iv)
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
    fn test_salsa20_encrypt_decrypt() {
        let key = b"test key for salsa20 cipher";
        let plaintext = b"Hello Salsa20!";

        let mut enc = Salsa20Cipher::new(key);
        let ciphertext = enc.process(plaintext);

        let mut dec = Salsa20Cipher::new(key);
        let decrypted = dec.process(&ciphertext);

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_salsa20_deterministic() {
        let key = b"my secret key";

        let mut c1 = Salsa20Cipher::new(key);
        let r1 = c1.process(b"test data");

        let mut c2 = Salsa20Cipher::new(key);
        let r2 = c2.process(b"test data");

        assert_eq!(r1, r2);
    }
}
