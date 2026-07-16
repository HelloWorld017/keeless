//! Generic stream cipher trait
//!
//! Abstraction over Salsa20 and ChaCha20 stream ciphers.

/// Stream cipher trait for KeePass inner encryption.
pub trait StreamCipher {
    /// Process data through the stream cipher.
    /// For stream ciphers, encryption and decryption are the same operation.
    fn process(&mut self, data: &[u8]) -> Vec<u8>;
}

// Re-export concrete implementations that implement StreamCipher
impl StreamCipher for super::salsa20_cipher::Salsa20Cipher {
    fn process(&mut self, data: &[u8]) -> Vec<u8> {
        self.process(data)
    }
}

impl StreamCipher for super::chacha20_cipher::ChaCha20Cipher {
    fn process(&mut self, data: &[u8]) -> Vec<u8> {
        self.process(data)
    }
}
