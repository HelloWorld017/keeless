//! Inner stream cipher for KDBX protected field encryption
//!
//! Salsa20 for KDBX 3.1, ChaCha20 for KDBX 4.0

use crate::crypto::{ChaCha20Cipher, Salsa20Cipher};
use crate::kdbx::file::header::CrsAlgorithm;
use crate::model::exception::{DatabaseError, DatabaseResult};

/// Inner stream cipher trait.
/// Processes protected field values sequentially across all fields in document order.
pub trait InnerStreamCipher {
    /// Encrypt/decrypt data in-place. Stream position advances by data.len().
    fn process(&mut self, data: &mut [u8]);
}

/// Salsa20 inner stream (KDBX 3.1).
/// Key is SHA-256 hashed, uses KeePass hardcoded IV.
pub struct Salsa20InnerStream {
    cipher: Salsa20Cipher,
}

impl Salsa20InnerStream {
    pub fn new(key: &[u8]) -> Self {
        Self {
            cipher: Salsa20Cipher::new(key),
        }
    }
}

impl InnerStreamCipher for Salsa20InnerStream {
    fn process(&mut self, data: &mut [u8]) {
        let processed = self.cipher.process(data);
        data.copy_from_slice(&processed);
    }
}

/// ChaCha20 inner stream (KDBX 4.0).
/// Key used directly, zero nonce.
pub struct ChaCha20InnerStream {
    cipher: ChaCha20Cipher,
}

impl ChaCha20InnerStream {
    pub fn new(key: &[u8]) -> DatabaseResult<Self> {
        let cipher = ChaCha20Cipher::new(key, &[0u8; 12])
            .map_err(|e| DatabaseError::DecryptionError(e.to_string()))?;
        Ok(Self { cipher })
    }
}

impl InnerStreamCipher for ChaCha20InnerStream {
    fn process(&mut self, data: &mut [u8]) {
        let processed = self.cipher.process(data);
        data.copy_from_slice(&processed);
    }
}

/// ARC4 (RC4 variant) inner stream.
/// Used for legacy KDBX compatibility (CrsAlgorithm::ArcFourVariant).
pub struct ArcFourInnerStream {
    state: [u8; 256],
    i: u8,
    j: u8,
}

impl ArcFourInnerStream {
    pub fn new(key: &[u8]) -> Self {
        let mut state = [0u8; 256];
        for (i, byte) in state.iter_mut().enumerate() {
            *byte = i as u8;
        }
        let mut j: u8 = 0;
        for i in 0..256 {
            j = j.wrapping_add(state[i]).wrapping_add(key[i % key.len()]);
            state.swap(i, j as usize);
        }
        Self { state, i: 0, j: 0 }
    }

    fn next_byte(&mut self) -> u8 {
        self.i = self.i.wrapping_add(1);
        self.j = self.j.wrapping_add(self.state[self.i as usize]);
        self.state.swap(self.i as usize, self.j as usize);
        let k = self.state[self.i as usize]
            .wrapping_add(self.state[self.j as usize]);
        self.state[k as usize]
    }
}

impl InnerStreamCipher for ArcFourInnerStream {
    fn process(&mut self, data: &mut [u8]) {
        for byte in data.iter_mut() {
            *byte ^= self.next_byte();
        }
    }
}

/// Create an inner stream cipher for the given CRS algorithm.
pub fn create_inner_stream(
    algorithm: CrsAlgorithm,
    key: &[u8],
) -> DatabaseResult<Box<dyn InnerStreamCipher>> {
    match algorithm {
        CrsAlgorithm::Salsa20 => Ok(Box::new(Salsa20InnerStream::new(key))),
        CrsAlgorithm::ChaCha20 => Ok(Box::new(ChaCha20InnerStream::new(key)?)),
        CrsAlgorithm::ArcFourVariant => Ok(Box::new(ArcFourInnerStream::new(key))),
        CrsAlgorithm::None => {
            Err(DatabaseError::Unsupported("No inner stream cipher configured".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_salsa20_sequential_fields() {
        let key = b"test_inner_stream_key";
        let mut stream = Salsa20InnerStream::new(key);

        let mut field1 = b"password1".to_vec();
        let mut field2 = b"secret2".to_vec();

        let original1 = field1.clone();
        let original2 = field2.clone();

        // Encrypt
        stream.process(&mut field1);
        stream.process(&mut field2);

        assert_ne!(field1, original1);
        assert_ne!(field2, original2);

        // Decrypt (re-create cipher to reset keystream position)
        let mut stream2 = Salsa20InnerStream::new(key);
        stream2.process(&mut field1);
        stream2.process(&mut field2);

        assert_eq!(field1, original1);
        assert_eq!(field2, original2);
    }

    #[test]
    fn test_chacha20_roundtrip() {
        let key = [0x42u8; 32];
        let mut stream = ChaCha20InnerStream::new(&key).unwrap();

        let mut data = b"sensitive data".to_vec();
        let original = data.clone();
        stream.process(&mut data);
        assert_ne!(data, original);

        let mut stream2 = ChaCha20InnerStream::new(&key).unwrap();
        stream2.process(&mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn test_arc4_roundtrip() {
        let key = b"arc4_test_key";
        let mut stream = ArcFourInnerStream::new(key);

        let mut data = b"protected value".to_vec();
        let original = data.clone();
        stream.process(&mut data);
        assert_ne!(data, original);

        let mut stream2 = ArcFourInnerStream::new(key);
        stream2.process(&mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn test_arc4_sequential_fields() {
        let key = b"multi_field_key";
        let mut stream = ArcFourInnerStream::new(key);

        let mut f1 = b"field1".to_vec();
        let mut f2 = b"field2".to_vec();
        let orig1 = f1.clone();
        let orig2 = f2.clone();

        stream.process(&mut f1);
        stream.process(&mut f2);

        let mut stream2 = ArcFourInnerStream::new(key);
        stream2.process(&mut f1);
        stream2.process(&mut f2);

        assert_eq!(f1, orig1);
        assert_eq!(f2, orig2);
    }
}
