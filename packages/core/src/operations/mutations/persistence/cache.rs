use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce, aead::Aead};
use sha2::{Digest, Sha256};

use super::MutationCoordinator;
use crate::{CoreError, Result, random_array};

const CACHE_MAGIC: &[u8; 8] = b"KLSCACHE";
const CACHE_VERSION: u8 = 1;
const CACHE_NONCE_LENGTH: usize = 24;
const CACHE_TAG_LENGTH: usize = 16;

impl MutationCoordinator {
    pub(crate) fn encode_cache(&self, database: &[u8]) -> Result<Vec<u8>> {
        let sequence = self.next_sequence;
        let nonce = random_array::<CACHE_NONCE_LENGTH>()?;
        let aad = cache_aad(self.database_id.as_bytes(), sequence, database);
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
        let aad = cache_aad(self.database_id.as_bytes(), sequence, database);
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
}

fn cache_aad(database_id: &[u8], sequence: u64, database: &[u8]) -> Vec<u8> {
    let digest = Sha256::digest(database);
    let mut aad = Vec::with_capacity(database_id.len() + digest.len() + 32);
    aad.extend_from_slice(b"keeless database cache\0");
    aad.extend_from_slice(database_id);
    aad.push(CACHE_VERSION);
    aad.extend_from_slice(&sequence.to_le_bytes());
    aad.extend_from_slice(&digest);
    aad
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DatabaseId;
    use keeless_kdbx::SecureArray;

    #[test]
    fn database_id_bytes_preserve_cache_authentication() {
        let raw = SecureArray::from_slice(&[9; 32]).unwrap();
        let bytes = b"local-file\0vault.kdbx".to_vec();
        let coordinator =
            MutationCoordinator::new(&raw, DatabaseId::new(bytes.clone()), 7).unwrap();
        let same = MutationCoordinator::new(&raw, DatabaseId::new(bytes), 0).unwrap();
        let cache = coordinator.encode_cache(b"encrypted-kdbx").unwrap();
        assert_eq!(
            same.decode_cache(&cache).unwrap(),
            (7, b"encrypted-kdbx".to_vec())
        );

        let other = MutationCoordinator::new(&raw, DatabaseId::new(b"other".to_vec()), 0).unwrap();
        assert!(matches!(
            other.decode_cache(&cache),
            Err(CoreError::InvalidCache)
        ));
    }
}
