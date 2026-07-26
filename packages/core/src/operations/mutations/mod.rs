use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce, aead::Aead};
use hkdf::Hkdf;
use keeless_kdbx::{
    CompositeKey, Database, DateInstant, Entry, EntryFieldId, EntryFieldUpdate,
    EntryPropertiesUpdate, Group, IconImage, IconImageStandard, IconUpdate, NodeId, SecureArray,
    kdbx::template::{self, TemplateCopyMode},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::{CoreError, KeelessCore, Result, random_array};

const JOURNAL_VERSION: u8 = 1;
const CACHE_MAGIC: &[u8; 8] = b"KLSCACHE";
const CACHE_VERSION: u8 = 1;
const CACHE_NONCE_LENGTH: usize = 24;
const CACHE_TAG_LENGTH: usize = 16;
const STORAGE_HKDF_INFO: &[u8] = b"keeless storage key v1";
const JOURNAL_HKDF_INFO: &[u8] = b"keeless mutation journal key v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Mutation {
    AddEntry {
        parent: NodeId,
        id: NodeId,
        timestamp_ms: i64,
    },
    AddEntryFromTemplate {
        parent: NodeId,
        template: NodeId,
        id: NodeId,
        link_field_id: Uuid,
        timestamp_ms: i64,
        preserve_protected: bool,
    },
    AddGroup {
        parent: NodeId,
        id: NodeId,
        timestamp_ms: i64,
    },
    DeleteEntry {
        id: NodeId,
        permanent: bool,
        recycle_bin_id: Uuid,
        timestamp_ms: i64,
    },
    DeleteGroup {
        id: NodeId,
        permanent: bool,
        recycle_bin_id: Uuid,
        timestamp_ms: i64,
    },
    MoveEntry {
        id: NodeId,
        parent: NodeId,
        timestamp_ms: i64,
    },
    MoveGroup {
        id: NodeId,
        parent: NodeId,
        index: usize,
        timestamp_ms: i64,
    },
    RenameGroup {
        id: NodeId,
        name: String,
        timestamp_ms: i64,
    },
    UpdateGroup {
        id: NodeId,
        name: String,
        standard_icon: u32,
        custom_icon: Option<Uuid>,
        timestamp_ms: i64,
    },
    UpdateEntry {
        id: NodeId,
        fields: Vec<JournalEntryField>,
        properties: Option<JournalEntryProperties>,
        new_custom_field_ids: Vec<Uuid>,
        timestamp_ms: i64,
    },
    UpdateTagStyles {
        value: String,
        timestamp_ms: i64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalEntryField {
    pub field_id: Option<String>,
    pub name: String,
    pub value: Option<String>,
    pub is_protected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalEntryProperties {
    pub override_url: String,
    pub tags: Vec<String>,
    pub expires: bool,
    pub expiry_time_ms: Option<i64>,
    pub standard_icon: Option<u32>,
    pub custom_icon: Option<Uuid>,
}

impl Drop for JournalEntryField {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

impl Drop for JournalEntryProperties {
    fn drop(&mut self) {
        self.override_url.zeroize();
        self.tags.zeroize();
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct JournalLine {
    version: u8,
    sequence: u64,
    nonce: String,
    ciphertext: String,
}

pub struct MutationCoordinator {
    key: SecureArray<32>,
    identity: Vec<u8>,
    next_sequence: u64,
    dirty: bool,
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
    pub fn new(raw_key: &SecureArray<32>, identity: Vec<u8>, next_sequence: u64) -> Result<Self> {
        let mut storage_key = [0; 32];
        raw_key.unlock(|raw| {
            Hkdf::<Sha256>::new(Some(&identity), raw)
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
            identity,
            next_sequence,
            dirty: false,
        })
    }

    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
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

    pub(crate) fn encode_cache(&self, database: &[u8]) -> Result<Vec<u8>> {
        let sequence = self.next_sequence;
        let nonce = random_array::<CACHE_NONCE_LENGTH>()?;
        let aad = cache_aad(&self.identity, sequence, database);
        let tag = self
            .key
            .unlock(|key| {
                XChaCha20Poly1305::new(key.into()).encrypt(
                    XNonce::from_slice(&nonce),
                    chacha20poly1305::aead::Payload {
                        msg: &[],
                        aad: &aad,
                    },
                )
            })?
            .map_err(|_| CoreError::Crypto)?;
        let mut cache = Vec::with_capacity(
            CACHE_MAGIC.len() + 1 + 8 + CACHE_NONCE_LENGTH + CACHE_TAG_LENGTH + database.len(),
        );
        cache.extend_from_slice(CACHE_MAGIC);
        cache.push(CACHE_VERSION);
        cache.extend_from_slice(&sequence.to_le_bytes());
        cache.extend_from_slice(&nonce);
        cache.extend_from_slice(&tag);
        cache.extend_from_slice(database);
        Ok(cache)
    }

    pub(crate) fn decode_cache(&self, cache: &[u8]) -> Result<(u64, Vec<u8>)> {
        let header_len = CACHE_MAGIC.len() + 1 + 8 + CACHE_NONCE_LENGTH + CACHE_TAG_LENGTH;
        if cache.len() <= header_len
            || &cache[..CACHE_MAGIC.len()] != CACHE_MAGIC
            || cache[CACHE_MAGIC.len()] != CACHE_VERSION
        {
            return Err(CoreError::InvalidCache);
        }
        let sequence_start = CACHE_MAGIC.len() + 1;
        let sequence = u64::from_le_bytes(
            cache[sequence_start..sequence_start + 8]
                .try_into()
                .map_err(|_| CoreError::InvalidCache)?,
        );
        let nonce_start = sequence_start + 8;
        let tag_start = nonce_start + CACHE_NONCE_LENGTH;
        let database_start = tag_start + CACHE_TAG_LENGTH;
        let nonce = XNonce::from_slice(&cache[nonce_start..tag_start]);
        let tag = &cache[tag_start..database_start];
        let database = &cache[database_start..];
        let aad = cache_aad(&self.identity, sequence, database);
        self.key
            .unlock(|key| {
                XChaCha20Poly1305::new(key.into()).decrypt(
                    nonce,
                    chacha20poly1305::aead::Payload {
                        msg: tag,
                        aad: &aad,
                    },
                )
            })?
            .map_err(|_| CoreError::InvalidCache)?;
        Ok((sequence, database.to_vec()))
    }

    fn encode(&self, mutation: &Mutation) -> Result<Vec<u8>> {
        let sequence = self.next_sequence;
        let nonce = random_array::<24>()?;
        let plaintext = Zeroizing::new(serde_json::to_vec(&mutation)?);
        let aad = aad(&self.identity, JOURNAL_VERSION, sequence);
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

/// Append one encrypted mutation and then synchronously commit its prepared state.
pub async fn mutate(
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

/// Decrypt and replay contiguous journal lines. Duplicate/gapped sequences are rejected.
pub fn replay_lines(
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
        let aad = aad(&coordinator.identity, line.version, line.sequence);
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

/// Apply one deterministic mutation during replay.
pub fn apply(database: &mut Database, mutation: &Mutation, key: &CompositeKey) -> Result<()> {
    let timestamp = |value| DateInstant::EpochMillis(value);
    let applied = match mutation {
        Mutation::AddEntry {
            parent,
            id,
            timestamp_ms,
        } => {
            if !database.can_add_entry(id, parent) {
                false
            } else {
                database.add_entry_validated(Entry::new_at(*id, timestamp(*timestamp_ms)), parent);
                true
            }
        }
        Mutation::AddEntryFromTemplate {
            parent,
            template: source,
            id,
            link_field_id,
            timestamp_ms,
            preserve_protected,
        } => template::instantiate_at(
            database,
            source,
            parent,
            *id,
            EntryFieldId::Custom(*link_field_id),
            timestamp(*timestamp_ms),
            if *preserve_protected {
                TemplateCopyMode::PreserveProtected
            } else {
                TemplateCopyMode::RedactProtected
            },
            (*preserve_protected).then_some(key),
        )?
        .is_some(),
        Mutation::AddGroup {
            parent,
            id,
            timestamp_ms,
        } => {
            if !database.can_add_group(id, parent) {
                false
            } else {
                let mut group = Group::new_at(*id, timestamp(*timestamp_ms));
                group.title = "Untitled Group".into();
                group.icon = IconImage::Standard(IconImageStandard::new(48));
                database.add_group_validated(group, parent);
                true
            }
        }
        Mutation::DeleteEntry {
            id,
            permanent,
            recycle_bin_id,
            timestamp_ms,
        } => database.delete_entry_at(id, *permanent, *recycle_bin_id, *timestamp_ms),
        Mutation::DeleteGroup {
            id,
            permanent,
            recycle_bin_id,
            timestamp_ms,
        } => database.delete_group_at(id, *permanent, *recycle_bin_id, *timestamp_ms),
        Mutation::MoveEntry {
            id,
            parent,
            timestamp_ms,
        } => database.reposition_entry_at(id, parent, timestamp(*timestamp_ms)),
        Mutation::MoveGroup {
            id,
            parent,
            index,
            timestamp_ms,
        } => database.reposition_group_at(id, parent, *index, timestamp(*timestamp_ms)),
        Mutation::RenameGroup {
            id,
            name,
            timestamp_ms,
        } => database.rename_group_at(id, name.clone(), timestamp(*timestamp_ms)),
        Mutation::UpdateGroup {
            id,
            name,
            standard_icon,
            custom_icon,
            timestamp_ms,
        } => database.update_group_at(
            id,
            name.clone(),
            IconUpdate {
                standard_id: *standard_icon,
                custom_uuid: *custom_icon,
            },
            timestamp(*timestamp_ms),
        ),
        Mutation::UpdateEntry {
            id,
            fields,
            properties,
            new_custom_field_ids,
            timestamp_ms,
        } => {
            let mut fields = fields
                .iter()
                .map(JournalEntryField::to_kdbx)
                .collect::<Result<Vec<_>>>()?;
            for field in &mut fields {
                if matches!(
                    field.field_id,
                    Some(EntryFieldId::Standard(
                        keeless_kdbx::StandardField::Password
                    ))
                ) && field.value.is_none()
                    && let Some(source) = database
                        .get_entry(id)
                        .and_then(|entry| entry.field(field.field_id.expect("password field ID")))
                    && !source.value().is_protected()
                {
                    field.value = Some(source.value().as_str().to_string());
                }
            }
            let properties = properties.as_ref().map(JournalEntryProperties::to_kdbx);
            database.update_entry_at(
                key,
                id,
                &fields,
                properties.as_ref(),
                new_custom_field_ids,
                timestamp(*timestamp_ms),
            )?
        }
        Mutation::UpdateTagStyles {
            value,
            timestamp_ms,
        } => {
            database.custom_data.set_at(
                crate::features::tag_styles::CUSTOM_DATA_KEY,
                value,
                Some(*timestamp_ms),
            );
            database.mark_modified();
            true
        }
    };
    if applied {
        Ok(())
    } else {
        Err(CoreError::InvalidJournal)
    }
}

impl JournalEntryField {
    fn to_kdbx(&self) -> Result<EntryFieldUpdate> {
        Ok(EntryFieldUpdate {
            field_id: self
                .field_id
                .as_deref()
                .map(str::parse)
                .transpose()
                .map_err(|_| CoreError::InvalidJournal)?,
            name: self.name.clone(),
            value: self.value.clone(),
            is_protected: self.is_protected,
        })
    }
}

impl JournalEntryProperties {
    fn to_kdbx(&self) -> EntryPropertiesUpdate {
        EntryPropertiesUpdate {
            override_url: self.override_url.clone(),
            tags: self.tags.clone(),
            expires: self.expires,
            expiry_time_ms: self.expiry_time_ms,
            icon: self.standard_icon.map(|standard_id| IconUpdate {
                standard_id,
                custom_uuid: self.custom_icon,
            }),
        }
    }
}

fn aad(identity: &[u8], version: u8, sequence: u64) -> Vec<u8> {
    let mut aad = Vec::with_capacity(identity.len() + 32);
    aad.extend_from_slice(b"keeless mutation journal\0");
    aad.extend_from_slice(identity);
    aad.push(version);
    aad.extend_from_slice(&sequence.to_le_bytes());
    aad
}

fn cache_aad(identity: &[u8], sequence: u64, database: &[u8]) -> Vec<u8> {
    let digest = Sha256::digest(database);
    let mut aad = Vec::with_capacity(identity.len() + digest.len() + 32);
    aad.extend_from_slice(b"keeless database cache\0");
    aad.extend_from_slice(identity);
    aad.push(CACHE_VERSION);
    aad.extend_from_slice(&sequence.to_le_bytes());
    aad.extend_from_slice(&digest);
    aad
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_entry_password_is_only_present_inside_encryption() {
        let raw = SecureArray::from_slice(&[7; 32]).unwrap();
        let coordinator = MutationCoordinator::new(&raw, b"identity".to_vec(), 0).unwrap();
        let mutation = Mutation::UpdateEntry {
            id: NodeId::from_uuid(Uuid::from_u128(1)),
            fields: vec![JournalEntryField {
                field_id: Some("standard:Password".into()),
                name: "Password".into(),
                value: Some("must-not-persist".into()),
                is_protected: true,
            }],
            properties: None,
            new_custom_field_ids: Vec::new(),
            timestamp_ms: 1,
        };

        let encoded = coordinator.encode(&mutation).unwrap();
        assert!(
            !encoded
                .windows(b"must-not-persist".len())
                .any(|window| window == b"must-not-persist")
        );
        let line: JournalLine = serde_json::from_slice(&encoded).unwrap();
        let nonce = URL_SAFE_NO_PAD.decode(line.nonce).unwrap();
        let ciphertext = URL_SAFE_NO_PAD.decode(line.ciphertext).unwrap();
        let plaintext = coordinator
            .key
            .unlock(|key| {
                XChaCha20Poly1305::new(key.into()).decrypt(
                    XNonce::from_slice(&nonce),
                    chacha20poly1305::aead::Payload {
                        msg: &ciphertext,
                        aad: &aad(&coordinator.identity, line.version, line.sequence),
                    },
                )
            })
            .unwrap()
            .unwrap();
        assert!(
            String::from_utf8(plaintext)
                .unwrap()
                .contains("must-not-persist")
        );
    }

    #[test]
    fn cache_watermark_is_bound_to_the_database_and_identity() {
        let raw = SecureArray::from_slice(&[9; 32]).unwrap();
        let coordinator = MutationCoordinator::new(&raw, b"identity".to_vec(), 7).unwrap();
        let cache = coordinator.encode_cache(b"encrypted-kdbx").unwrap();
        assert_eq!(
            coordinator.decode_cache(&cache).unwrap(),
            (7, b"encrypted-kdbx".to_vec())
        );

        let mut changed_sequence = cache.clone();
        changed_sequence[CACHE_MAGIC.len() + 1] ^= 1;
        assert!(matches!(
            coordinator.decode_cache(&changed_sequence),
            Err(CoreError::InvalidCache)
        ));

        let other = MutationCoordinator::new(&raw, b"other".to_vec(), 0).unwrap();
        assert!(matches!(
            other.decode_cache(&cache),
            Err(CoreError::InvalidCache)
        ));
    }
}
