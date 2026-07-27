use std::any::Any;
use std::collections::HashMap;

use keeless_kdbx::{CompositeKey, Database, NodeId, passkey_credential_ids};
use sha2::{Digest, Sha256};

use super::CoreExtension;
use crate::{CoreError, Result, random_array};

/// Credential-ID presence index valid only while its database is unlocked.
pub(crate) struct PasskeyExtension {
    salt: [u8; 32],
    credential_hashes: HashMap<NodeId, [u8; 32]>,
}

impl PasskeyExtension {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            salt: random_array()?,
            credential_hashes: HashMap::new(),
        })
    }

    pub(crate) fn matches_any(&self, entry_id: &NodeId, credential_ids: &[Vec<u8>]) -> bool {
        let Some(expected) = self.credential_hashes.get(entry_id) else {
            return false;
        };
        credential_ids
            .iter()
            .map(|credential_id| self.hash(credential_id))
            .any(|candidate| candidate == *expected)
    }

    pub(crate) fn insert(&mut self, entry_id: NodeId, credential_id: &[u8]) {
        let hash = self.hash(credential_id);
        self.credential_hashes.insert(entry_id, hash);
    }

    fn hash(&self, credential_id: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.salt);
        hasher.update(credential_id);
        hasher.finalize().into()
    }
}

impl CoreExtension for PasskeyExtension {
    fn unlock(&mut self, database: &Database, key: &CompositeKey) -> Result<()> {
        let credential_hashes = passkey_credential_ids(database, key)
            .map_err(CoreError::from)?
            .into_iter()
            .map(|(entry_id, credential_id)| (entry_id, self.hash(&credential_id)))
            .collect();
        self.credential_hashes = credential_hashes;
        Ok(())
    }

    fn lock(&mut self) {
        self.credential_hashes.clear();
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
