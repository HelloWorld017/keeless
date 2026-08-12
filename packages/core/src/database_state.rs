use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use hkdf::Hkdf;
use keeless_kdbx::SecureArray;
use keeless_lesswire::{Error as WireError, Result as WireResult, StateStore, WireFuture};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::{CoreError, DatabaseId, DatabasePersistence, Result, random_array};

pub(crate) const CONFIG_RECORD: &str = "config";
pub(crate) const CORE_WIRE_RECORD: &str = "core-wire-state";
const STATE_VERSION: u8 = 1;
const ROOT_HKDF_INFO: &[u8] = b"keeless database state root v1";
const CONFIG_HKDF_INFO: &[u8] = b"keeless database state config v1";
const CORE_WIRE_HKDF_INFO: &[u8] = b"keeless database state core wire v1";
const MAX_STATE_RECORD_SIZE: usize = 128 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EncryptedRecord {
    version: u8,
    nonce: String,
    ciphertext: String,
}

#[derive(Clone)]
pub(crate) struct EncryptedDatabaseStateStore {
    persistence: Arc<dyn DatabasePersistence>,
    database_id: DatabaseId,
    root_key: Arc<SecureArray<32>>,
}

impl EncryptedDatabaseStateStore {
    pub(crate) fn new(
        raw_key: &SecureArray<32>,
        persistence: Arc<dyn DatabasePersistence>,
        database_id: DatabaseId,
    ) -> Result<Self> {
        let mut root = [0; 32];
        raw_key.unlock(|raw| {
            Hkdf::<Sha256>::new(Some(database_id.as_bytes()), raw)
                .expand(ROOT_HKDF_INFO, &mut root)
                .map_err(|_| CoreError::Crypto)
        })??;
        Ok(Self {
            persistence,
            database_id,
            root_key: Arc::new(SecureArray::from_array_mut(&mut root)?),
        })
    }

    pub(crate) async fn load_record(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        validate_record_name(name)?;
        let Some(encoded) = self.persistence.read_state_record(name).await? else {
            return Ok(None);
        };
        if encoded.len() > MAX_STATE_RECORD_SIZE {
            return Err(CoreError::InvalidConfig(
                "encrypted state record is too large".into(),
            ));
        }
        let record: EncryptedRecord = serde_json::from_slice(&encoded)
            .map_err(|_| CoreError::InvalidConfig("invalid encrypted state record".into()))?;
        if record.version != STATE_VERSION {
            return Err(CoreError::InvalidConfig(
                "unsupported encrypted state version".into(),
            ));
        }
        let nonce = URL_SAFE_NO_PAD
            .decode(record.nonce)
            .map_err(|_| CoreError::InvalidConfig("invalid encrypted state nonce".into()))?;
        let nonce: [u8; 24] = nonce
            .try_into()
            .map_err(|_| CoreError::InvalidConfig("invalid encrypted state nonce".into()))?;
        let ciphertext = URL_SAFE_NO_PAD
            .decode(record.ciphertext)
            .map_err(|_| CoreError::InvalidConfig("invalid encrypted state ciphertext".into()))?;
        let key = self.record_key(name)?;
        let plaintext = XChaCha20Poly1305::new((&*key).into())
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &self.aad(name),
                },
            )
            .map_err(|_| {
                CoreError::InvalidConfig("encrypted state authentication failed".into())
            })?;
        Ok(Some(Zeroizing::new(plaintext)))
    }

    pub(crate) async fn save_record(&self, name: &str, plaintext: &[u8]) -> Result<()> {
        validate_record_name(name)?;
        if plaintext.len() > MAX_STATE_RECORD_SIZE {
            return Err(CoreError::InvalidConfig(
                "encrypted state record is too large".into(),
            ));
        }
        let nonce = random_array::<24>()?;
        let key = self.record_key(name)?;
        let ciphertext = XChaCha20Poly1305::new((&*key).into())
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: &self.aad(name),
                },
            )
            .map_err(|_| CoreError::Crypto)?;
        let encoded = Zeroizing::new(serde_json::to_vec(&EncryptedRecord {
            version: STATE_VERSION,
            nonce: URL_SAFE_NO_PAD.encode(nonce),
            ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
        })?);
        self.persistence.write_state_record(name, &encoded).await
    }

    fn record_key(&self, name: &str) -> Result<Zeroizing<[u8; 32]>> {
        let info = match name {
            CONFIG_RECORD => CONFIG_HKDF_INFO,
            CORE_WIRE_RECORD => CORE_WIRE_HKDF_INFO,
            _ => {
                return Err(CoreError::InvalidConfig(
                    "invalid encrypted state record name".into(),
                ));
            }
        };
        let mut key = Zeroizing::new([0; 32]);
        self.root_key
            .unlock(|root| Hkdf::<Sha256>::new(None, root).expand(info, key.as_mut()))?
            .map_err(|_| CoreError::Crypto)?;
        Ok(key)
    }

    fn aad(&self, name: &str) -> Vec<u8> {
        let mut aad = Vec::with_capacity(self.database_id.as_bytes().len() + name.len() + 32);
        aad.extend_from_slice(b"keeless database state\0");
        aad.push(STATE_VERSION);
        aad.extend_from_slice(self.database_id.as_bytes());
        aad.push(0);
        aad.extend_from_slice(name.as_bytes());
        aad
    }
}

impl StateStore for EncryptedDatabaseStateStore {
    fn load(&self) -> WireFuture<'_, WireResult<Option<Vec<u8>>>> {
        Box::pin(async move {
            self.load_record(CORE_WIRE_RECORD)
                .await
                .map(|value| value.map(|bytes| bytes.to_vec()))
                .map_err(core_to_wire)
        })
    }

    fn save<'a>(&'a self, state: &'a [u8]) -> WireFuture<'a, WireResult<()>> {
        Box::pin(async move {
            self.save_record(CORE_WIRE_RECORD, state)
                .await
                .map_err(core_to_wire)
        })
    }
}

fn validate_record_name(name: &str) -> Result<()> {
    match name {
        CONFIG_RECORD | CORE_WIRE_RECORD => Ok(()),
        _ => Err(CoreError::InvalidConfig(
            "invalid encrypted state record name".into(),
        )),
    }
}

fn core_to_wire(error: CoreError) -> WireError {
    WireError::Host(error.to_string())
}
