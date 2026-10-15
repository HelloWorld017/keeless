use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use keeless_kdbx::{CompositeCredentials, CompositeKey, KdfParameters};
use keeless_secure_types::{SecureArray, SecureBytes};
use zeroize::Zeroizing;

use crate::{CoreError, Result, random_array};

const CREDENTIAL_AAD: &[u8] = b"keeless-credential-v2";

pub(crate) struct CredentialVault {
    current: WrappedKey,
    next: Option<PreparedNextKey>,
}

struct PreparedNextKey {
    key: WrappedKey,
    parameters: KdfParameters,
    source_fingerprint: [u8; 32],
}

struct WrappedKey {
    wrapping_key: SecureArray<32>,
    nonce: [u8; 24],
    ciphertext: SecureBytes,
}

impl CredentialVault {
    pub fn wrap(key: &CompositeKey) -> Result<Self> {
        Ok(Self {
            current: WrappedKey::wrap(key)?,
            next: None,
        })
    }

    /// Called only with credentials that have authenticated the current database.
    /// No password or pre-KDF material survives this operation.
    pub(crate) fn prepare(
        key: &CompositeKey,
        credentials: &CompositeCredentials,
        parameters: &KdfParameters,
    ) -> Result<Self> {
        if !key.matches(parameters) {
            return Err(keeless_kdbx::DatabaseError::KdfParametersMismatch.into());
        }
        let (parameters, next_key) = keeless_kdbx::prepare_database_key(
            parameters,
            credentials,
            keeless_kdbx::RekeyOptions::default(),
        )?;
        Ok(Self {
            current: WrappedKey::wrap(key)?,
            next: Some(PreparedNextKey {
                key: WrappedKey::wrap(&next_key)?,
                parameters,
                source_fingerprint: key.kdf_fingerprint(),
            }),
        })
    }

    pub(crate) fn restore_key(&self) -> Result<CompositeKey> {
        self.current.restore_key()
    }

    /// Restoring does not consume the slot: failed writes may retry the same key.
    pub(crate) fn restore_next(&self) -> Result<Option<(KdfParameters, CompositeKey)>> {
        let Some(next) = &self.next else {
            return Ok(None);
        };
        let current = self.restore_key()?;
        let key = next.key.restore_key()?;
        if current.kdf_fingerprint() != next.source_fingerprint
            || !key.matches(&next.parameters)
            || key.kdf_fingerprint() == current.kdf_fingerprint()
        {
            return Err(CoreError::Crypto);
        }
        Ok(Some((next.parameters.clone(), key)))
    }

    pub(crate) fn has_prepared_next(&self) -> bool {
        self.next.is_some()
    }

    #[cfg(test)]
    pub fn key_matches(&self, expected: &[u8; 32]) -> Result<bool> {
        self.restore_key()?
            .with_key(|value| value == expected)
            .map_err(Into::into)
    }
}

impl WrappedKey {
    fn wrap(key: &CompositeKey) -> Result<Self> {
        let mut wrapping = random_array::<32>()?;
        let wrapping_key = SecureArray::from_array_mut(&mut wrapping)?;
        let nonce = random_array::<24>()?;
        let ciphertext = wrapping_key
            .unlock(|wrapping| {
                key.with_key(|transformed| {
                    let mut payload = Zeroizing::new([0u8; 64]);
                    payload[..32].copy_from_slice(transformed);
                    payload[32..].copy_from_slice(&key.kdf_fingerprint());
                    XChaCha20Poly1305::new(wrapping.into()).encrypt(
                        XNonce::from_slice(&nonce),
                        Payload {
                            msg: payload.as_ref(),
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
        if plaintext.len() != 64 {
            return Err(CoreError::Crypto);
        }
        let fingerprint: [u8; 32] = plaintext[32..].try_into().map_err(|_| CoreError::Crypto)?;
        Ok(CompositeKey::from_derived_key(
            SecureArray::from_slice(&plaintext[..32])?,
            fingerprint,
        ))
    }
}

impl std::fmt::Debug for CredentialVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialVault")
            .field("credential", &"[REDACTED]")
            .finish()
    }
}
