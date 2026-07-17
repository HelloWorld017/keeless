//! Twofish block cipher implementation
//!

use keeless_secure_types::SecureArray;
use twofish::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use twofish::Twofish;

use super::{CipherMode, CryptoError, CryptoResult};

const BLOCK_SIZE: usize = 16;
const KEY_SIZE: usize = 32;

/// Twofish-256 cipher in CBC mode.
pub struct TwofishCipher {
    key: SecureArray<32>,
    iv: [u8; BLOCK_SIZE],
    mode: CipherMode,
    padding: bool,
}

impl TwofishCipher {
    pub fn new(mode: CipherMode, key: &[u8], iv: &[u8]) -> CryptoResult<Self> {
        if key.len() != KEY_SIZE {
            return Err(CryptoError::InvalidKeyLength {
                expected: KEY_SIZE,
                got: key.len(),
            });
        }
        if iv.len() != BLOCK_SIZE {
            return Err(CryptoError::InvalidIvLength {
                expected: BLOCK_SIZE,
                got: iv.len(),
            });
        }
        let mut iv_arr = [0u8; BLOCK_SIZE];
        iv_arr.copy_from_slice(iv);
        Ok(Self {
            key: SecureArray::from_slice(key)?,
            iv: iv_arr,
            mode,
            padding: true,
        })
    }

    pub fn without_padding(mut self) -> Self {
        self.padding = false;
        self
    }

    pub fn process(&self, data: &[u8]) -> CryptoResult<Vec<u8>> {
        self.key.unlock(|key| {
            let cipher = Twofish::new_from_slice(key)
                .map_err(|e| CryptoError::EncryptionFailed(e.to_string()))?;

            match self.mode {
                CipherMode::Encrypt => {
                    let padded = if self.padding {
                        pkcs7_pad(data, BLOCK_SIZE)
                    } else {
                        data.to_vec()
                    };
                    if padded.len() % BLOCK_SIZE != 0 {
                        return Err(CryptoError::InvalidDataLength(format!(
                            "Data must be multiple of {BLOCK_SIZE}, got {}",
                            padded.len()
                        )));
                    }
                    let mut prev = self.iv;
                    let mut result = Vec::with_capacity(padded.len());
                    for chunk in padded.chunks(BLOCK_SIZE) {
                        let mut block: [u8; BLOCK_SIZE] = chunk.try_into().map_err(|_| {
                            CryptoError::InvalidDataLength("Block alignment error".into())
                        })?;
                        xor_in_place(&mut block, &prev);
                        cipher.encrypt_block((&mut block).into());
                        result.extend_from_slice(&block);
                        prev = block;
                    }
                    Ok(result)
                }
                CipherMode::Decrypt => {
                    if data.len() % BLOCK_SIZE != 0 {
                        return Err(CryptoError::InvalidDataLength(format!(
                            "Ciphertext must be multiple of {BLOCK_SIZE}, got {}",
                            data.len()
                        )));
                    }
                    let mut prev = self.iv;
                    let mut decrypted = Vec::with_capacity(data.len());
                    for chunk in data.chunks(BLOCK_SIZE) {
                        let ct: [u8; BLOCK_SIZE] = chunk.try_into().map_err(|_| {
                            CryptoError::InvalidDataLength("Block alignment error".into())
                        })?;
                        let mut block = ct;
                        cipher.decrypt_block((&mut block).into());
                        xor_in_place(&mut block, &prev);
                        decrypted.extend_from_slice(&block);
                        prev = ct;
                    }
                    if self.padding {
                        pkcs7_unpad(&mut decrypted)?;
                    }
                    Ok(decrypted)
                }
            }
        })?
    }
}

fn xor_in_place(a: &mut [u8; 16], b: &[u8; 16]) {
    for (ai, bi) in a.iter_mut().zip(b.iter()) {
        *ai ^= bi;
    }
}

fn pkcs7_pad(data: &[u8], block_size: usize) -> Vec<u8> {
    let padding_len = block_size - (data.len() % block_size);
    let mut padded = data.to_vec();
    padded.extend(vec![padding_len as u8; padding_len]);
    padded
}

fn pkcs7_unpad(data: &mut Vec<u8>) -> CryptoResult<()> {
    if data.is_empty() {
        return Err(CryptoError::DecryptionFailed("Empty data".into()));
    }
    let pad_byte = *data.last().expect("checked non-empty above") as usize;
    if pad_byte == 0 || pad_byte > BLOCK_SIZE || pad_byte > data.len() {
        return Err(CryptoError::DecryptionFailed(format!(
            "Invalid PKCS7 padding: {pad_byte}"
        )));
    }
    for &b in data.iter().rev().take(pad_byte) {
        if b as usize != pad_byte {
            return Err(CryptoError::DecryptionFailed(
                "Invalid PKCS7 padding".into(),
            ));
        }
    }
    data.truncate(data.len() - pad_byte);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_twofish_encrypt_decrypt() {
        let key = [0x42u8; 32];
        let iv = [0x13u8; 16];
        let plaintext = b"Twofish test data here!";

        let enc = TwofishCipher::new(CipherMode::Encrypt, &key, &iv).unwrap();
        let ct = enc.process(plaintext).unwrap();
        assert_ne!(ct.as_slice(), plaintext.as_slice());

        let dec = TwofishCipher::new(CipherMode::Decrypt, &key, &iv).unwrap();
        let pt = dec.process(&ct).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn test_twofish_no_padding() {
        let key = [0x42u8; 32];
        let iv = [0x13u8; 16];
        let plaintext = [0xABu8; 32];

        let enc = TwofishCipher::new(CipherMode::Encrypt, &key, &iv)
            .unwrap()
            .without_padding();
        let ct = enc.process(&plaintext).unwrap();

        let dec = TwofishCipher::new(CipherMode::Decrypt, &key, &iv)
            .unwrap()
            .without_padding();
        let pt = dec.process(&ct).unwrap();
        assert_eq!(pt, plaintext);
    }
}
