use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use keeless_kdbx::CompositeKey;
use keeless_secure_types::{SecureArray, SecureBytes};
use keeless_sync::{FileHandle, SyncReport};
use zeroize::Zeroizing;

use crate::{CoreError, Result, random_array};

const CREDENTIAL_AAD: &[u8] = b"keeless-credential-v1";

pub(crate) struct CredentialVault {
    wrapping_key: SecureArray<32>,
    nonce: [u8; 24],
    ciphertext: SecureBytes,
}

impl CredentialVault {
    pub fn wrap(raw_key: &SecureArray<32>) -> Result<Self> {
        let mut wrapping = random_array::<32>()?;
        let wrapping_key = SecureArray::from_array_mut(&mut wrapping)?;
        let nonce = random_array::<24>()?;
        let ciphertext = wrapping_key
            .unlock(|key| {
                raw_key.unlock(|raw| {
                    XChaCha20Poly1305::new(key.into()).encrypt(
                        XNonce::from_slice(&nonce),
                        Payload {
                            msg: raw,
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

    pub async fn sync(&self, handle: &mut FileHandle) -> Result<SyncReport> {
        let key = self.restore_key()?;
        Ok(handle.sync(&key).await?)
    }

    fn restore_key(&self) -> Result<CompositeKey> {
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
        let secure = SecureArray::from_slice(&plaintext)?;
        Ok(CompositeKey::from_raw_key(secure))
    }

    #[cfg(test)]
    pub fn raw_key_matches(&self, expected: &[u8; 32]) -> Result<bool> {
        self.restore_key()?
            .build_raw_key()?
            .unlock(|value| value == expected)
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
