//! Credential-scoped protection for decrypted KDBX entry strings.

use std::collections::HashMap;
use std::sync::Arc;

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use secure_types::{SecureArray, SecureBytes};
use sha2::Sha256;
use zeroize::{Zeroize, Zeroizing};

use crate::kdbx::kdf::{create_kdf, KdfParameters};
use crate::model::core::node::NodeId;
use crate::model::db::composite_key::CompositeKey;
use crate::model::exception::{DatabaseError, DatabaseResult};

const ROOT_INFO: &[u8] = b"keeless/kdbx/memory/root/v1";
const ENTRY_INFO: &[u8] = b"keeless/kdbx/memory/entry/v1";
const VALUE_AAD: &[u8] = b"keeless/kdbx/memory/value/v1";
const VERIFIER_AAD: &[u8] = b"keeless/kdbx/memory/verifier/v1";
const VERIFIER_PLAINTEXT: &[u8] = b"keeless-memory-protection";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum MemoryField {
    Title,
    UserName,
    Password,
    Url,
    Notes,
    Custom(String),
}

impl MemoryField {
    fn encode(&self, output: &mut Vec<u8>) {
        match self {
            Self::Title => output.push(1),
            Self::UserName => output.push(2),
            Self::Password => output.push(3),
            Self::Url => output.push(4),
            Self::Notes => output.push(5),
            Self::Custom(name) => {
                output.push(6);
                output.extend_from_slice(&(name.len() as u64).to_be_bytes());
                output.extend_from_slice(name.as_bytes());
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct MemoryProtectionContext {
    id: [u8; 16],
    kdf_parameters: KdfParameters,
    salt: [u8; 32],
    verifier_nonce: [u8; 24],
    verifier: Vec<u8>,
}

impl std::fmt::Debug for MemoryProtectionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryProtectionContext")
            .field("id", &self.id)
            .field("kdf_uuid", &self.kdf_parameters.kdf_uuid)
            .finish_non_exhaustive()
    }
}

impl MemoryProtectionContext {
    pub(crate) fn create(
        composite_key: &CompositeKey,
        mut kdf_parameters: KdfParameters,
    ) -> DatabaseResult<(Arc<Self>, SecureArray<32>)> {
        let kdf = create_kdf(&kdf_parameters.kdf_uuid)
            .ok_or_else(|| DatabaseError::InvalidFormat("Unknown memory-protection KDF".into()))?;
        if kdf_parameters.get_byte_array("S").is_none() {
            kdf.randomize(&mut kdf_parameters)?;
        }

        let raw_key = composite_key.build_raw_key()?;
        let transformed =
            SecureBytes::from_vec(raw_key.unlock(|value| kdf.transform(value, &kdf_parameters))??)?;

        let mut id = [0u8; 16];
        let mut salt = [0u8; 32];
        let mut verifier_nonce = [0u8; 24];
        fill_random(&mut id)?;
        fill_random(&mut salt)?;
        fill_random(&mut verifier_nonce)?;
        let root = transformed.unlock_slice(|value| derive_root(value, &salt))??;
        let verifier = root.unlock(|value| {
            XChaCha20Poly1305::new(value.into())
                .encrypt(
                    XNonce::from_slice(&verifier_nonce),
                    Payload {
                        msg: VERIFIER_PLAINTEXT,
                        aad: VERIFIER_AAD,
                    },
                )
                .map_err(|_| {
                    DatabaseError::EncryptionError("memory key verification setup failed".into())
                })
        })??;

        Ok((
            Arc::new(Self {
                id,
                kdf_parameters,
                salt,
                verifier_nonce,
                verifier,
            }),
            root,
        ))
    }

    fn unlock(&self, composite_key: &CompositeKey) -> DatabaseResult<SecureArray<32>> {
        let raw_key = composite_key.build_raw_key()?;
        let kdf = create_kdf(&self.kdf_parameters.kdf_uuid)
            .ok_or_else(|| DatabaseError::InvalidFormat("Unknown memory-protection KDF".into()))?;
        let transformed = SecureBytes::from_vec(
            raw_key.unlock(|value| kdf.transform(value, &self.kdf_parameters))??,
        )?;
        let root = transformed.unlock_slice(|value| derive_root(value, &self.salt))??;
        root.unlock(|value| {
            XChaCha20Poly1305::new(value.into())
                .decrypt(
                    XNonce::from_slice(&self.verifier_nonce),
                    Payload {
                        msg: &self.verifier,
                        aad: VERIFIER_AAD,
                    },
                )
                .map_err(|_| DatabaseError::InvalidCredentials)
        })??;
        Ok(root)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct EncryptedValue {
    pub(crate) context: Arc<MemoryProtectionContext>,
    nonce: [u8; 24],
    ciphertext: Vec<u8>,
}

impl std::fmt::Debug for EncryptedValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EncryptedValue([REDACTED])")
    }
}

impl Drop for EncryptedValue {
    fn drop(&mut self) {
        self.ciphertext.zeroize();
        self.nonce.zeroize();
    }
}

impl EncryptedValue {
    pub(crate) fn encrypt(
        context: Arc<MemoryProtectionContext>,
        root: &[u8; 32],
        entry_id: NodeId,
        field: &MemoryField,
        plaintext: &[u8],
    ) -> DatabaseResult<Self> {
        let entry_key = derive_entry_key(root, entry_id)?;
        let mut nonce = [0u8; 24];
        fill_random(&mut nonce)?;
        let aad = value_aad(entry_id, field);
        let ciphertext = entry_key.unlock(|value| {
            XChaCha20Poly1305::new(value.into())
                .encrypt(
                    XNonce::from_slice(&nonce),
                    Payload {
                        msg: plaintext,
                        aad: &aad,
                    },
                )
                .map_err(|_| DatabaseError::EncryptionError("memory protection failed".into()))
        })??;
        Ok(Self {
            context,
            nonce,
            ciphertext,
        })
    }

    pub(crate) fn decrypt(
        &self,
        root: &[u8; 32],
        entry_id: NodeId,
        field: &MemoryField,
    ) -> DatabaseResult<Zeroizing<Vec<u8>>> {
        let entry_key = derive_entry_key(root, entry_id)?;
        let aad = value_aad(entry_id, field);
        let plaintext = entry_key.unlock(|value| {
            XChaCha20Poly1305::new(value.into())
                .decrypt(
                    XNonce::from_slice(&self.nonce),
                    Payload {
                        msg: &self.ciphertext,
                        aad: &aad,
                    },
                )
                .map_err(|_| {
                    DatabaseError::DecryptionError(
                        "protected memory value authentication failed".into(),
                    )
                })
        })??;
        Ok(Zeroizing::new(plaintext))
    }

    pub(crate) fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }
}

pub(crate) struct MemoryUnlockSession<'a> {
    composite_key: &'a CompositeKey,
    roots: HashMap<[u8; 16], SecureArray<32>>,
}

impl<'a> MemoryUnlockSession<'a> {
    pub(crate) fn new(composite_key: &'a CompositeKey) -> Self {
        Self {
            composite_key,
            roots: HashMap::new(),
        }
    }

    pub(crate) fn with_root<T>(
        &mut self,
        context: &Arc<MemoryProtectionContext>,
        use_root: impl FnOnce(&[u8; 32]) -> DatabaseResult<T>,
    ) -> DatabaseResult<T> {
        if !self.roots.contains_key(&context.id) {
            self.roots
                .insert(context.id, context.unlock(self.composite_key)?);
        }
        self.roots[&context.id].unlock(use_root)?
    }
}

fn derive_root(transformed: &[u8], salt: &[u8; 32]) -> DatabaseResult<SecureArray<32>> {
    let hkdf = Hkdf::<Sha256>::new(Some(salt), transformed);
    let mut root = SecureArray::zeroed()?;
    root.unlock_mut(|value| {
        hkdf.expand(ROOT_INFO, value)
            .map_err(|_| DatabaseError::EncryptionError("memory root derivation failed".into()))
    })??;
    Ok(root)
}

fn derive_entry_key(root: &[u8; 32], entry_id: NodeId) -> DatabaseResult<SecureArray<32>> {
    let hkdf = Hkdf::<Sha256>::new(None, root);
    let mut info = Vec::with_capacity(ENTRY_INFO.len() + 17);
    info.extend_from_slice(ENTRY_INFO);
    encode_node_id(entry_id, &mut info);
    let mut key = SecureArray::zeroed()?;
    key.unlock_mut(|value| {
        hkdf.expand(&info, value)
            .map_err(|_| DatabaseError::EncryptionError("entry key derivation failed".into()))
    })??;
    Ok(key)
}

fn value_aad(entry_id: NodeId, field: &MemoryField) -> Vec<u8> {
    let mut aad = Vec::with_capacity(VALUE_AAD.len() + 64);
    aad.extend_from_slice(VALUE_AAD);
    encode_node_id(entry_id, &mut aad);
    field.encode(&mut aad);
    aad
}

fn encode_node_id(id: NodeId, output: &mut Vec<u8>) {
    match id {
        NodeId::Uuid(uuid) => {
            output.push(1);
            output.extend_from_slice(uuid.as_bytes());
        }
        NodeId::Int(value) => {
            output.push(2);
            output.extend_from_slice(&value.to_be_bytes());
        }
    }
}

fn fill_random(output: &mut [u8]) -> DatabaseResult<()> {
    getrandom::getrandom(output).map_err(|err| {
        DatabaseError::EncryptionError(format!("secure random generation failed: {err}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kdbx::kdf::aes_kdf::AES_KDF_UUID;

    fn parameters() -> KdfParameters {
        let mut parameters = KdfParameters::new(AES_KDF_UUID);
        parameters.set_byte_array("S", &[0x42; 32]);
        parameters.set_uint64("R", 1);
        parameters
    }

    #[test]
    fn protected_value_requires_matching_credentials_and_context() {
        let key = CompositeKey::new().with_password(b"correct horse").unwrap();
        let (context, root) = MemoryProtectionContext::create(&key, parameters()).unwrap();
        let entry_id = NodeId::new_uuid();
        let plaintext = b"a secret that must not remain in the model";
        let encrypted = root
            .unlock(|root| {
                EncryptedValue::encrypt(
                    context.clone(),
                    root,
                    entry_id,
                    &MemoryField::Password,
                    plaintext,
                )
            })
            .unwrap()
            .unwrap();

        assert!(!encrypted
            .ciphertext()
            .windows(plaintext.len())
            .any(|window| window == plaintext));

        let mut unlock = MemoryUnlockSession::new(&key);
        unlock
            .with_root(&context, |root| {
                assert_eq!(
                    encrypted
                        .decrypt(root, entry_id, &MemoryField::Password)
                        .unwrap()
                        .as_slice(),
                    plaintext
                );
                assert!(encrypted
                    .decrypt(root, NodeId::new_uuid(), &MemoryField::Password)
                    .is_err());
                assert!(encrypted
                    .decrypt(root, entry_id, &MemoryField::Notes)
                    .is_err());
                Ok(())
            })
            .unwrap();

        let wrong_key = CompositeKey::new().with_password(b"wrong horse").unwrap();
        assert!(MemoryUnlockSession::new(&wrong_key)
            .with_root(&context, |_| Ok(()))
            .is_err());
    }

    #[test]
    fn repeated_encryption_uses_fresh_nonces() {
        let key = CompositeKey::new().with_password(b"password").unwrap();
        let (context, root) = MemoryProtectionContext::create(&key, parameters()).unwrap();
        let entry_id = NodeId::new_uuid();
        let (first, second) = root
            .unlock(|root| {
                Ok::<_, DatabaseError>((
                    EncryptedValue::encrypt(
                        context.clone(),
                        root,
                        entry_id,
                        &MemoryField::Password,
                        b"same",
                    )?,
                    EncryptedValue::encrypt(
                        context,
                        root,
                        entry_id,
                        &MemoryField::Password,
                        b"same",
                    )?,
                ))
            })
            .unwrap()
            .unwrap();
        assert_ne!(first.ciphertext, second.ciphertext);
        assert_ne!(first.nonce, second.nonce);
    }
}
