use ed25519_dalek::SigningKey;
use keeless_kdbx::SecureArray;
use keeless_schema::MessageFrame;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::{
    KeelessCore, Result,
    protocol::{
        FRAME_TIMESTAMP_TOLERANCE_MS, FRAME_VERSION, NONCE_CACHE_CAPACITY, PublicKeyBundle,
        parse_public_key_bundle, public_key_bundle, valid_ephemeral_key, valid_nonce, verify_frame,
    },
};

pub(crate) struct Identity {
    pub(crate) signing_seed: SecureArray<32>,
    pub(crate) x25519_secret: SecureArray<32>,
}

impl Identity {
    pub(crate) fn signing_key(&self) -> Result<SigningKey> {
        self.signing_seed
            .unlock(SigningKey::from_bytes)
            .map_err(Into::into)
    }

    pub(crate) fn x25519(&self) -> Result<StaticSecret> {
        self.x25519_secret
            .unlock(|secret| StaticSecret::from(*secret))
            .map_err(Into::into)
    }

    pub(crate) fn bundle(&self) -> Result<String> {
        let signing = self.signing_key()?;
        let encryption = PublicKey::from(&self.x25519()?);
        Ok(public_key_bundle(&signing, &encryption))
    }
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Identity([REDACTED])")
    }
}

impl KeelessCore {
    pub fn public_key_bundle(&self) -> Result<String> {
        self.identity.bundle()
    }

    pub(crate) async fn authenticate_frame(
        &mut self,
        frame: &MessageFrame,
    ) -> Result<Option<PublicKeyBundle>> {
        let is_handshake = frame.payload.is_none();
        let ephemeral_is_valid = if is_handshake {
            frame.ephemeral_public_key.is_none()
        } else {
            valid_ephemeral_key(frame.ephemeral_public_key.as_deref())
        };
        if frame.version != FRAME_VERSION || !valid_nonce(&frame.nonce) || !ephemeral_is_valid {
            return Ok(None);
        }

        let now = self.clock.now_millis();
        if frame.timestamp.abs_diff(now) > FRAME_TIMESTAMP_TOLERANCE_MS as u64 {
            return Ok(None);
        }
        let Some(sender) = parse_public_key_bundle(&frame.public_key) else {
            return Ok(None);
        };
        if !verify_frame(frame, &sender) {
            return Ok(None);
        }
        if !self
            .identity
            .x25519()?
            .diffie_hellman(&sender.encryption)
            .was_contributory()
        {
            return Ok(None);
        }
        if !is_handshake && !self.approved_clients.contains(&frame.public_key) {
            return Ok(None);
        }

        let is_approved = self.approved_clients.contains(&frame.public_key);
        let accepted_at = self.clock.monotonic_millis();
        self.nonce_cache.retain(|_, observed_at| {
            accepted_at.saturating_sub(*observed_at) <= FRAME_TIMESTAMP_TOLERANCE_MS as u64
        });
        if self.nonce_cache.contains_key(&frame.nonce)
            || self.nonce_cache.len() >= NONCE_CACHE_CAPACITY
        {
            return Ok(None);
        }
        self.nonce_cache.insert(frame.nonce.clone(), accepted_at);

        if is_handshake && !is_approved {
            if !self.approval_provider.approve(&frame.public_key).await? {
                return Ok(None);
            }
            self.approved_clients.push(frame.public_key.clone());
            if let Err(error) = self.persist().await {
                self.approved_clients.pop();
                return Err(error);
            }
        }

        Ok(Some(sender))
    }
}
