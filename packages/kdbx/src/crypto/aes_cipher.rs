//! AES cipher implementation (CBC/ECB mode with PKCS5/PKCS7 padding)
//!

use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes256;
use secure_types::SecureArray;

use super::{BlockMode, CipherMode, CryptoError, CryptoResult};

const AES_BLOCK_SIZE: usize = 16;
const AES_256_KEY_SIZE: usize = 32;

/// AES-256 cipher wrapper supporting CBC and ECB modes.
pub struct AesCipher {
    key: SecureArray<32>,
    iv: [u8; AES_BLOCK_SIZE],
    mode: CipherMode,
    block_mode: BlockMode,
    padding: bool,
}

impl AesCipher {
    /// Create a new AES-256 CBC cipher.
    pub fn new(mode: CipherMode, key: &[u8], iv: &[u8]) -> CryptoResult<Self> {
        if key.len() != AES_256_KEY_SIZE {
            return Err(CryptoError::InvalidKeyLength {
                expected: AES_256_KEY_SIZE,
                got: key.len(),
            });
        }
        if iv.len() != AES_BLOCK_SIZE {
            return Err(CryptoError::InvalidIvLength {
                expected: AES_BLOCK_SIZE,
                got: iv.len(),
            });
        }
        let mut iv_arr = [0u8; AES_BLOCK_SIZE];
        iv_arr.copy_from_slice(iv);
        Ok(Self {
            key: SecureArray::from_slice(key)?,
            iv: iv_arr,
            mode,
            block_mode: BlockMode::CBC,
            padding: true,
        })
    }

    /// Create AES cipher without padding.
    pub fn without_padding(mut self) -> Self {
        self.padding = false;
        self
    }

    /// Create an AES-256 ECB cipher (used in AES-KDF key transformation).
    pub fn new_ecb(mode: CipherMode, key: &[u8]) -> CryptoResult<Self> {
        if key.len() != AES_256_KEY_SIZE {
            return Err(CryptoError::InvalidKeyLength {
                expected: AES_256_KEY_SIZE,
                got: key.len(),
            });
        }
        Ok(Self {
            key: SecureArray::from_slice(key)?,
            iv: [0u8; AES_BLOCK_SIZE],
            mode,
            block_mode: BlockMode::ECB,
            padding: false,
        })
    }

    /// Process data through the cipher.
    pub fn process(&self, data: &[u8]) -> CryptoResult<Vec<u8>> {
        match self.block_mode {
            BlockMode::CBC => self.process_cbc(data),
            BlockMode::ECB => self.process_ecb(data),
            BlockMode::CTR => Err(CryptoError::EncryptionFailed(
                "CTR not supported for AES here".into(),
            )),
        }
    }

    fn process_cbc(&self, data: &[u8]) -> CryptoResult<Vec<u8>> {
        self.key.unlock(|key| {
            let cipher = Aes256::new_from_slice(key)
                .map_err(|e| CryptoError::EncryptionFailed(e.to_string()))?;

            match self.mode {
                CipherMode::Encrypt => {
                    let padded = if self.padding {
                        pkcs7_pad(data, AES_BLOCK_SIZE)
                    } else {
                        data.to_vec()
                    };
                    if padded.len() % AES_BLOCK_SIZE != 0 {
                        return Err(CryptoError::InvalidDataLength(format!(
                            "CBC data must be multiple of {AES_BLOCK_SIZE}, got {}",
                            padded.len()
                        )));
                    }
                    let mut prev = self.iv;
                    let mut result = Vec::with_capacity(padded.len());
                    for chunk in padded.chunks(AES_BLOCK_SIZE) {
                        let mut block: [u8; AES_BLOCK_SIZE] = chunk.try_into().map_err(|_| {
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
                    if data.len() % AES_BLOCK_SIZE != 0 {
                        return Err(CryptoError::InvalidDataLength(format!(
                            "CBC ciphertext must be multiple of {AES_BLOCK_SIZE}, got {}",
                            data.len()
                        )));
                    }
                    let mut prev = self.iv;
                    let mut decrypted = Vec::with_capacity(data.len());
                    for chunk in data.chunks(AES_BLOCK_SIZE) {
                        let ct: [u8; AES_BLOCK_SIZE] = chunk.try_into().map_err(|_| {
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

    fn process_ecb(&self, data: &[u8]) -> CryptoResult<Vec<u8>> {
        self.key.unlock(|key| {
            let cipher = Aes256::new_from_slice(key)
                .map_err(|e| CryptoError::EncryptionFailed(e.to_string()))?;

            if data.len() % AES_BLOCK_SIZE != 0 {
                return Err(CryptoError::InvalidDataLength(format!(
                    "ECB data must be multiple of {AES_BLOCK_SIZE}, got {}",
                    data.len()
                )));
            }

            let mut result = Vec::with_capacity(data.len());
            for chunk in data.chunks(AES_BLOCK_SIZE) {
                let mut block: [u8; AES_BLOCK_SIZE] = chunk
                    .try_into()
                    .map_err(|_| CryptoError::InvalidDataLength("Block alignment error".into()))?;
                match self.mode {
                    CipherMode::Encrypt => cipher.encrypt_block((&mut block).into()),
                    CipherMode::Decrypt => cipher.decrypt_block((&mut block).into()),
                }
                result.extend_from_slice(&block);
            }
            Ok(result)
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
    if pad_byte == 0 || pad_byte > AES_BLOCK_SIZE || pad_byte > data.len() {
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
    fn test_aes256_cbc_encrypt_decrypt() {
        let key = [0x42u8; 32];
        let iv = [0x13u8; 16];
        let plaintext = b"Hello, KeePassDX Rust rewrite!";

        let enc = AesCipher::new(CipherMode::Encrypt, &key, &iv).unwrap();
        let ct = enc.process(plaintext).unwrap();
        assert_ne!(ct.as_slice(), plaintext.as_slice());
        assert_eq!(ct.len() % 16, 0);

        let dec = AesCipher::new(CipherMode::Decrypt, &key, &iv).unwrap();
        let pt = dec.process(&ct).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn test_aes256_ecb_encrypt_decrypt() {
        let key = [0xABu8; 32];
        let plaintext = [0x42u8; 32];

        let enc = AesCipher::new_ecb(CipherMode::Encrypt, &key).unwrap();
        let ct = enc.process(&plaintext).unwrap();
        assert_eq!(ct.len(), 32);

        let dec = AesCipher::new_ecb(CipherMode::Decrypt, &key).unwrap();
        let pt = dec.process(&ct).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn test_aes_cbc_no_padding() {
        let key = [0x42u8; 32];
        let iv = [0x13u8; 16];
        let plaintext = [0xABu8; 48];

        let enc = AesCipher::new(CipherMode::Encrypt, &key, &iv)
            .unwrap()
            .without_padding();
        let ct = enc.process(&plaintext).unwrap();

        let dec = AesCipher::new(CipherMode::Decrypt, &key, &iv)
            .unwrap()
            .without_padding();
        let pt = dec.process(&ct).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn test_invalid_key_length() {
        assert!(AesCipher::new(CipherMode::Encrypt, &[0u8; 16], &[0u8; 16]).is_err());
    }
}
