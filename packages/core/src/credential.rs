use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use keeless_kdbx::CompositeKey;
use keeless_secure_types::{SecureArray, SecureBytes};
use zeroize::Zeroizing;

use crate::{CoreError, Result, random_array};

const CREDENTIAL_AAD: &[u8] = b"keeless-credential-v2";

pub(crate) struct CredentialVault {
    wrapping_key: SecureArray<32>,
    nonce: [u8; 24],
    ciphertext: SecureBytes,
}

impl CredentialVault {
    pub fn wrap(key: &CompositeKey) -> Result<Self> {
        let mut wrapping = random_array::<32>()?;
        let wrapping_key = SecureArray::from_array_mut(&mut wrapping)?;
        let nonce = random_array::<24>()?;
        let ciphertext = wrapping_key
            .unlock(|wrapping| {
                key.with_key(|transformed| {
                    let mut payload = [0u8; 64];
                    payload[..32].copy_from_slice(transformed);
                    payload[32..].copy_from_slice(&key.kdf_fingerprint());
                    XChaCha20Poly1305::new(wrapping.into()).encrypt(
                        XNonce::from_slice(&nonce),
                        Payload {
                            msg: &payload,
                            aad: CREDENTIAL_AAD,
                        },
                    )
                })
            })??
            .map_err(|_| CoreError::Crypto)?;
        Ok(Self {
            wrapping_key,
            nonce,
            ciphertext: SecureBytes::from_vec(ciphertext)?,
        })
    }

    pub(crate) fn restore_key(&self) -> Result<CompositeKey> {
        let plaintext = self
            .wrapping_key
            .unlock(|key| {
                self.ciphertext.unlock_slice(|ciphertext| {
                    XChaCha20Poly1305::new(key.into()).decrypt(
                        XNonce::from_slice(&self.nonce),
                        Payload {
                            msg: ciphertext,
                            aad: CREDENTIAL_AAD,
                        },
                    )
                })
            })??
            .map_err(|_| CoreError::Crypto)?;
        let plaintext = Zeroizing::new(plaintext);
        if plaintext.len() != 64 {
            return Err(CoreError::Crypto);
        }
        let key: [u8; 32] = plaintext[..32].try_into().map_err(|_| CoreError::Crypto)?;
        let fingerprint: [u8; 32] = plaintext[32..].try_into().map_err(|_| CoreError::Crypto)?;
        Ok(CompositeKey::from_derived_key(
            SecureArray::from_slice(&key)?,
            fingerprint,
        ))
    }

    #[cfg(test)]
    pub fn key_matches(&self, expected: &[u8; 32]) -> Result<bool> {
        self.restore_key()?
            .with_key(|value| value == expected)
            .map_err(Into::into)
    }
}

impl std::fmt::Debug for CredentialVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialVault")
            .field("credential", &"[REDACTED]")
            .finish()
    }
}
