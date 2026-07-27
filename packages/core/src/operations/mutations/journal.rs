use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce, aead::Aead};
use hkdf::Hkdf;
use keeless_kdbx::{CompositeKey, Database, SecureArray};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::Zeroizing;

use super::{Mutation, apply};
use crate::{CoreError, DatabaseId, KeelessCore, Result, random_array};

pub(super) const JOURNAL_VERSION: u8 = 1;
const STORAGE_HKDF_INFO: &[u8] = b"keeless storage key v1";
const JOURNAL_HKDF_INFO: &[u8] = b"keeless mutation journal key v1";

#[derive(Debug, Serialize, Deserialize)]
struct JournalLine {
    version: u8,
    sequence: u64,
    nonce: String,
    ciphertext: String,
}

pub(crate) struct MutationCoordinator {
    pub(super) key: SecureArray<32>,
    pub(super) database_id: DatabaseId,
    pub(super) next_sequence: u64,
    pub(super) dirty: bool,
}

impl std::fmt::Debug for MutationCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MutationCoordinator")
            .field("next_sequence", &self.next_sequence)
            .field("dirty", &self.dirty)
            .finish_non_exhaustive()
    }
}

impl MutationCoordinator {
    pub(crate) fn new(
        raw_key: &SecureArray<32>,
        database_id: DatabaseId,
        next_sequence: u64,
    ) -> Result<Self> {
        let mut storage_key = [0; 32];
        raw_key.unlock(|raw| {
            Hkdf::<Sha256>::new(Some(database_id.as_bytes()), raw)
                .expand(STORAGE_HKDF_INFO, &mut storage_key)
                .map_err(|_| CoreError::Crypto)
        })??;
        let mut journal_key = [0; 32];
        Hkdf::<Sha256>::new(None, &storage_key)
            .expand(JOURNAL_HKDF_INFO, &mut journal_key)
            .map_err(|_| CoreError::Crypto)?;
        storage_key.fill(0);
        Ok(Self {
            key: SecureArray::from_array_mut(&mut journal_key)?,
            database_id,
            next_sequence,
            dirty: false,
        })
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub(crate) fn mark_clean(&mut self) {
        self.dirty = false;
    }

    pub(crate) fn set_sequence(&mut self, sequence: u64) {
        self.next_sequence = sequence;
    }

    fn encode(&self, mutation: &Mutation) -> Result<Vec<u8>> {
        let sequence = self.next_sequence;
        let nonce = random_array::<24>()?;
        let plaintext = Zeroizing::new(serde_json::to_vec(mutation)?);
        let aad = journal_aad(self.database_id.as_bytes(), JOURNAL_VERSION, sequence);
        let ciphertext = self
            .key
            .unlock(|key| {
                XChaCha20Poly1305::new(key.into()).encrypt(
                    XNonce::from_slice(&nonce),
                    chacha20poly1305::aead::Payload {
                        msg: &plaintext,
                        aad: &aad,
                    },
                )
            })?
            .map_err(|_| CoreError::Crypto)?;
        Ok(serde_json::to_vec(&JournalLine {
            version: JOURNAL_VERSION,
            sequence,
            nonce: URL_SAFE_NO_PAD.encode(nonce),
            ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
        })?)
    }
}

pub(super) async fn mutate(
    core: &mut KeelessCore,
    mutation: &Mutation,
    commit: impl FnOnce(&mut Database),
) -> Result<()> {
    let persistence = core.persistence.clone();
    if let Some(persistence) = persistence {
        let line = core
            .journal
            .as_ref()
            .ok_or(CoreError::DatabaseLocked)?
            .encode(mutation)?;
        persistence.append_journal(&line).await?;
        core.journal
            .as_mut()
            .expect("journal checked")
            .next_sequence += 1;
        core.journal.as_mut().expect("journal checked").dirty = true;
    }
    core.handle
        .as_mut()
        .ok_or(CoreError::DatabaseLocked)?
        .commit_prepared(commit);
    core.dirty = true;
    Ok(())
}

pub(crate) fn replay_lines(
    coordinator: &mut MutationCoordinator,
    database: &mut Database,
    composite_key: &CompositeKey,
    lines: &[Vec<u8>],
) -> Result<()> {
    for encoded in lines {
        let line: JournalLine = serde_json::from_slice(encoded)?;
        if line.version != JOURNAL_VERSION {
            return Err(CoreError::InvalidJournal);
        }
        if line.sequence < coordinator.next_sequence {
            continue;
        }
        if line.sequence != coordinator.next_sequence {
            return Err(CoreError::InvalidJournal);
        }
        let nonce = URL_SAFE_NO_PAD
            .decode(line.nonce)
            .map_err(|_| CoreError::InvalidJournal)?;
        let ciphertext = URL_SAFE_NO_PAD
            .decode(line.ciphertext)
            .map_err(|_| CoreError::InvalidJournal)?;
        let nonce: [u8; 24] = nonce.try_into().map_err(|_| CoreError::InvalidJournal)?;
        let aad = journal_aad(
            coordinator.database_id.as_bytes(),
            line.version,
            line.sequence,
        );
        let plaintext = Zeroizing::new(
            coordinator
                .key
                .unlock(|key| {
                    XChaCha20Poly1305::new(key.into()).decrypt(
                        XNonce::from_slice(&nonce),
                        chacha20poly1305::aead::Payload {
                            msg: &ciphertext,
                            aad: &aad,
                        },
                    )
                })?
                .map_err(|_| CoreError::InvalidJournal)?,
        );
        let mutation: Mutation = serde_json::from_slice(&plaintext)?;
        apply(database, &mutation, composite_key)?;
        coordinator.next_sequence += 1;
        coordinator.dirty = true;
    }
    Ok(())
}

fn journal_aad(database_id: &[u8], version: u8, sequence: u64) -> Vec<u8> {
    let mut aad = Vec::with_capacity(database_id.len() + 32);
    aad.extend_from_slice(b"keeless mutation journal\0");
    aad.extend_from_slice(database_id);
    aad.push(version);
    aad.extend_from_slice(&sequence.to_le_bytes());
    aad
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::mutations::{Mutation, update_entry};
    use uuid::Uuid;

    fn decrypt(
        coordinator: &MutationCoordinator,
        encoded: &[u8],
    ) -> std::result::Result<Vec<u8>, ()> {
        let line: JournalLine = serde_json::from_slice(encoded).unwrap();
        let nonce = URL_SAFE_NO_PAD.decode(line.nonce).unwrap();
        let ciphertext = URL_SAFE_NO_PAD.decode(line.ciphertext).unwrap();
        coordinator
            .key
            .unlock(|key| {
                XChaCha20Poly1305::new(key.into()).decrypt(
                    XNonce::from_slice(&nonce),
                    chacha20poly1305::aead::Payload {
                        msg: &ciphertext,
                        aad: &journal_aad(
                            coordinator.database_id.as_bytes(),
                            line.version,
                            line.sequence,
                        ),
                    },
                )
            })
            .unwrap()
            .map_err(|_| ())
    }

    #[test]
    fn update_entry_password_is_only_present_inside_encryption() {
        let raw = SecureArray::from_slice(&[7; 32]).unwrap();
        let coordinator =
            MutationCoordinator::new(&raw, DatabaseId::new(b"database-id".to_vec()), 0).unwrap();
        let mutation = Mutation::UpdateEntry(update_entry::Mutation {
            id: keeless_kdbx::NodeId::from_uuid(Uuid::from_u128(1)),
            fields: vec![update_entry::JournalEntryField {
                field_id: Some("standard:Password".into()),
                name: "Password".into(),
                value: Some("must-not-persist".into()),
                is_protected: true,
            }],
            properties: None,
            new_custom_field_ids: Vec::new(),
            timestamp_ms: 1,
        });

        let encoded = coordinator.encode(&mutation).unwrap();
        assert!(
            !encoded
                .windows(b"must-not-persist".len())
                .any(|window| window == b"must-not-persist")
        );
        let plaintext = decrypt(&coordinator, &encoded).unwrap();
        assert!(
            String::from_utf8(plaintext)
                .unwrap()
                .contains("must-not-persist")
        );
    }

    #[test]
    fn database_id_bytes_preserve_journal_authentication() {
        let raw = SecureArray::from_slice(&[8; 32]).unwrap();
        let bytes = b"local-file\0vault.kdbx".to_vec();
        let coordinator =
            MutationCoordinator::new(&raw, DatabaseId::new(bytes.clone()), 5).unwrap();
        let mutation = Mutation::UpdateEntry(update_entry::Mutation {
            id: keeless_kdbx::NodeId::from_uuid(Uuid::from_u128(1)),
            fields: Vec::new(),
            properties: None,
            new_custom_field_ids: Vec::new(),
            timestamp_ms: 1,
        });
        let encoded = coordinator.encode(&mutation).unwrap();

        let same = MutationCoordinator::new(&raw, DatabaseId::new(bytes), 0).unwrap();
        assert!(decrypt(&same, &encoded).is_ok());
        let other = MutationCoordinator::new(&raw, DatabaseId::new(b"other".to_vec()), 0).unwrap();
        assert!(decrypt(&other, &encoded).is_err());
    }
}
