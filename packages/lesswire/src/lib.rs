//! Versioned authenticated encryption for untrusted Keeless relays.

use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use specta::Type;
use thiserror::Error;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

mod transfer;
pub use transfer::{
    MAX_TRANSFER_CHUNK_SIZE, MAX_TRANSFER_SIZE, TRANSFER_MAGIC, TRANSFER_TTL_MS, TransferError,
    TransferId, TransferOwner, TransferRegistry,
};

pub const FRAME_VERSION: u8 = 1;
pub const FRAME_TIMESTAMP_TOLERANCE_MS: i64 = 500;
pub const NONCE_CACHE_CAPACITY: usize = 2048;
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;
const FRAME_TRANSCRIPT_PREFIX: &str = "keeless-frame-v1";
const PAYLOAD_HEADER_PREFIX: &str = "keeless-payload-header-v1";
const PAYLOAD_HKDF_INFO: &[u8] = b"keeless-payload-v1";
const STATE_VERSION: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageFrame {
    pub version: u8,
    pub timestamp: i64,
    pub nonce: String,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub ephemeral_public_key: Option<String>,
    pub public_key: String,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub payload: Option<String>,
    pub signature: String,
}

fn deserialize_nullable<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("wire host error: {0}")]
    Host(String),
    #[error("invalid persisted wire state: {0}")]
    InvalidState(String),
    #[error("cryptographic operation failed")]
    Crypto,
    #[error("encrypted frame exceeds {MAX_FRAME_SIZE} bytes")]
    FrameTooLarge,
    #[error("wire serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(not(target_arch = "wasm32"))]
pub type WireFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
#[cfg(target_arch = "wasm32")]
pub type WireFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

#[cfg(not(target_arch = "wasm32"))]
pub trait ProviderRequirements: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync + ?Sized> ProviderRequirements for T {}
#[cfg(target_arch = "wasm32")]
pub trait ProviderRequirements {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> ProviderRequirements for T {}

/// Persists only wire identity and approvals granted by `ApprovalProvider`.
pub trait StateStore: ProviderRequirements {
    fn load(&self) -> WireFuture<'_, Result<Option<Vec<u8>>>>;
    fn save<'a>(&'a self, state: &'a [u8]) -> WireFuture<'a, Result<()>>;
}

/// Requests an explicit UI/user decision for a previously unknown client.
pub trait ApprovalProvider: ProviderRequirements {
    fn approve(&self, public_key_bundle: &str) -> WireFuture<'_, Result<bool>>;
}

pub trait Clock: ProviderRequirements {
    fn now_millis(&self) -> i64;
    fn monotonic_millis(&self) -> u64;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64
    }

    fn monotonic_millis(&self) -> u64 {
        use std::sync::OnceLock;
        use std::time::Instant;
        static ORIGIN: OnceLock<Instant> = OnceLock::new();
        ORIGIN
            .get_or_init(Instant::now)
            .elapsed()
            .as_millis()
            .min(u64::MAX as u128) as u64
    }
}

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Identity {
    signing_seed: [u8; 32],
    encryption_secret: [u8; 32],
}

impl Identity {
    pub fn generate() -> Result<Self> {
        Ok(Self {
            signing_seed: random_array()?,
            encryption_secret: random_array()?,
        })
    }

    pub fn from_secrets(signing_seed: [u8; 32], encryption_secret: [u8; 32]) -> Self {
        Self {
            signing_seed,
            encryption_secret,
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 64 {
            return Err(Error::InvalidState("invalid identity length".into()));
        }
        let mut signing_seed = [0; 32];
        let mut encryption_secret = [0; 32];
        signing_seed.copy_from_slice(&bytes[..32]);
        encryption_secret.copy_from_slice(&bytes[32..]);
        Ok(Self::from_secrets(signing_seed, encryption_secret))
    }

    /// Exports secret identity material for storage in a protected host store.
    pub fn to_bytes(&self) -> Zeroizing<[u8; 64]> {
        let mut bytes = Zeroizing::new([0; 64]);
        bytes[..32].copy_from_slice(&self.signing_seed);
        bytes[32..].copy_from_slice(&self.encryption_secret);
        bytes
    }

    pub fn public_key_bundle(&self) -> String {
        let signing = SigningKey::from_bytes(&self.signing_seed);
        let encryption = PublicKey::from(&StaticSecret::from(self.encryption_secret));
        public_key_bundle(&signing, &encryption)
    }

    fn signing_key(&self) -> SigningKey {
        SigningKey::from_bytes(&self.signing_seed)
    }

    fn encryption_key(&self) -> StaticSecret {
        StaticSecret::from(self.encryption_secret)
    }
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Identity([REDACTED])")
    }
}

#[derive(Clone)]
pub struct PublicKeyBundle {
    signing: VerifyingKey,
    encryption: PublicKey,
    encoded: String,
}

impl PublicKeyBundle {
    pub fn parse(value: &str) -> Option<Self> {
        let mut parts = value.split('.');
        if parts.next()? != "v1" {
            return None;
        }
        let signing: [u8; 32] = decode_canonical(parts.next()?)?.try_into().ok()?;
        let encryption: [u8; 32] = decode_canonical(parts.next()?)?.try_into().ok()?;
        if parts.next().is_some() || encryption == [0; 32] {
            return None;
        }
        let signing = VerifyingKey::from_bytes(&signing).ok()?;
        if signing.is_weak() {
            return None;
        }
        Some(Self {
            signing,
            encryption: PublicKey::from(encryption),
            encoded: value.into(),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.encoded
    }
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistedState {
    version: u8,
    signing_seed: String,
    encryption_secret: String,
    approved_client_bundles: Vec<String>,
}

pub struct ServerHost {
    pub store: Arc<dyn StateStore>,
    pub approval_provider: Arc<dyn ApprovalProvider>,
    pub clock: Arc<dyn Clock>,
    /// Preconfigured approvals valid for this process only; these are never persisted.
    pub runtime_approved_clients: Vec<String>,
}

pub struct Server {
    identity: Identity,
    persisted_approved: Vec<String>,
    runtime_approved: Vec<String>,
    nonce_cache: HashMap<String, u64>,
    host: ServerHost,
    transfers: TransferRegistry,
}

impl Server {
    pub async fn new(host: ServerHost) -> Result<Self> {
        let loaded = host.store.load().await?;
        let (identity, persisted_approved, generated) = match loaded {
            Some(bytes) => {
                let bytes = Zeroizing::new(bytes);
                let state: PersistedState = serde_json::from_slice(&bytes)
                    .map_err(|error| Error::InvalidState(error.to_string()))?;
                if state.version != STATE_VERSION {
                    return Err(Error::InvalidState("unsupported version".into()));
                }
                let signing_seed = decode_32(&state.signing_seed)?;
                let encryption_secret = decode_32(&state.encryption_secret)?;
                validate_bundles(&state.approved_client_bundles)?;
                (
                    Identity::from_secrets(*signing_seed, *encryption_secret),
                    state.approved_client_bundles.clone(),
                    false,
                )
            }
            None => (Identity::generate()?, Vec::new(), true),
        };
        validate_bundles(&host.runtime_approved_clients)?;
        let transfers = TransferRegistry::new(host.clock.clone());
        let server = Self {
            identity,
            persisted_approved,
            runtime_approved: host.runtime_approved_clients.clone(),
            nonce_cache: HashMap::new(),
            host,
            transfers,
        };
        if generated {
            server.persist().await?;
        }
        Ok(server)
    }

    pub fn public_key_bundle(&self) -> String {
        self.identity.public_key_bundle()
    }

    pub fn transfers(&self) -> TransferRegistry {
        self.transfers.clone()
    }

    /// Adds a validated approval for this server process without persisting it.
    pub fn add_runtime_approval(&mut self, bundle: &str) -> Result<()> {
        let bundle = PublicKeyBundle::parse(bundle)
            .ok_or_else(|| Error::InvalidState("invalid approved client bundle".into()))?;
        if !self.is_approved(bundle.as_str()) {
            self.runtime_approved.push(bundle.as_str().to_owned());
        }
        Ok(())
    }

    /// Replaces all process-local approvals with one validated client bundle.
    pub fn replace_runtime_approval(&mut self, bundle: &str) -> Result<()> {
        let bundle = PublicKeyBundle::parse(bundle)
            .ok_or_else(|| Error::InvalidState("invalid approved client bundle".into()))?;
        self.runtime_approved.clear();
        self.runtime_approved.push(bundle.as_str().to_owned());
        Ok(())
    }

    pub async fn handle_frame<F, Fut, E>(
        &mut self,
        frame: &MessageFrame,
        handler: F,
    ) -> std::result::Result<Option<MessageFrame>, HandleError<E>>
    where
        F: FnOnce(TransferOwner, Zeroizing<Vec<u8>>) -> Fut,
        Fut: Future<Output = std::result::Result<Option<Vec<u8>>, E>>,
    {
        if frame_size(frame) > MAX_FRAME_SIZE {
            return Ok(None);
        }
        let Some(sender) = self.authenticate(frame).await.map_err(HandleError::Wire)? else {
            return Ok(None);
        };
        if frame.payload.is_none() {
            return handshake_frame(
                self.host.clock.now_millis(),
                self.identity.public_key_bundle(),
                &self.identity.signing_key(),
            )
            .map(Some)
            .map_err(HandleError::Wire);
        }
        let Some(plaintext) = decrypt_frame(frame, &self.identity.encryption_key()) else {
            return Ok(None);
        };
        let owner = TransferOwner::new(sender.as_str());
        if plaintext.first() == Some(&TRANSFER_MAGIC) {
            let response = self.transfers.handle_packet(owner, &plaintext).ok();
            return response
                .map(|response| {
                    encrypt_frame(
                        self.host.clock.now_millis(),
                        &response,
                        self.identity.public_key_bundle(),
                        &self.identity.signing_key(),
                        &sender.encryption,
                    )
                })
                .transpose()
                .map_err(HandleError::Wire);
        }
        let Some(response) = handler(owner, Zeroizing::new(plaintext))
            .await
            .map_err(HandleError::Handler)?
        else {
            return Ok(None);
        };
        encrypt_frame(
            self.host.clock.now_millis(),
            &response,
            self.identity.public_key_bundle(),
            &self.identity.signing_key(),
            &sender.encryption,
        )
        .map(Some)
        .map_err(HandleError::Wire)
    }

    async fn authenticate(&mut self, frame: &MessageFrame) -> Result<Option<PublicKeyBundle>> {
        let handshake = frame.payload.is_none();
        if frame.version != FRAME_VERSION
            || !valid_nonce(&frame.nonce)
            || if handshake {
                frame.ephemeral_public_key.is_some()
            } else {
                !valid_ephemeral_key(frame.ephemeral_public_key.as_deref())
            }
            || frame.timestamp.abs_diff(self.host.clock.now_millis())
                > FRAME_TIMESTAMP_TOLERANCE_MS as u64
        {
            return Ok(None);
        }
        let Some(sender) = PublicKeyBundle::parse(&frame.public_key) else {
            return Ok(None);
        };
        if !verify_frame(frame, &sender)
            || !self
                .identity
                .encryption_key()
                .diffie_hellman(&sender.encryption)
                .was_contributory()
        {
            return Ok(None);
        }
        let approved = self.is_approved(&frame.public_key);
        if !handshake && !approved {
            return Ok(None);
        }
        let accepted_at = self.host.clock.monotonic_millis();
        self.nonce_cache.retain(|_, observed_at| {
            accepted_at.saturating_sub(*observed_at) <= FRAME_TIMESTAMP_TOLERANCE_MS as u64
        });
        if self.nonce_cache.contains_key(&frame.nonce)
            || self.nonce_cache.len() >= NONCE_CACHE_CAPACITY
        {
            return Ok(None);
        }
        self.nonce_cache.insert(frame.nonce.clone(), accepted_at);
        if handshake && !approved {
            if !self
                .host
                .approval_provider
                .approve(&frame.public_key)
                .await?
            {
                return Ok(None);
            }
            self.persisted_approved.push(frame.public_key.clone());
            if let Err(error) = self.persist().await {
                self.persisted_approved.pop();
                return Err(error);
            }
        }
        Ok(Some(sender))
    }

    fn is_approved(&self, bundle: &str) -> bool {
        self.persisted_approved.iter().any(|value| value == bundle)
            || self.runtime_approved.iter().any(|value| value == bundle)
    }

    async fn persist(&self) -> Result<()> {
        let state = PersistedState {
            version: STATE_VERSION,
            signing_seed: URL_SAFE_NO_PAD.encode(self.identity.signing_seed),
            encryption_secret: URL_SAFE_NO_PAD.encode(self.identity.encryption_secret),
            approved_client_bundles: self.persisted_approved.clone(),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&state)?);
        self.host.store.save(&bytes).await
    }
}

#[derive(Debug, Error)]
pub enum HandleError<E> {
    #[error(transparent)]
    Wire(Error),
    #[error("payload handler failed")]
    Handler(E),
}

pub struct Client {
    identity: Identity,
    trusted_server: Option<PublicKeyBundle>,
    nonce_cache: HashMap<String, u64>,
    clock: Arc<dyn Clock>,
}

impl Client {
    pub fn new(
        identity: Identity,
        trusted_server: Option<&str>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self> {
        let trusted_server = trusted_server
            .map(|value| {
                PublicKeyBundle::parse(value)
                    .ok_or_else(|| Error::InvalidState("invalid trusted server bundle".into()))
            })
            .transpose()?;
        Ok(Self {
            identity,
            trusted_server,
            nonce_cache: HashMap::new(),
            clock,
        })
    }

    pub fn public_key_bundle(&self) -> String {
        self.identity.public_key_bundle()
    }

    pub fn handshake_frame(&self) -> Result<MessageFrame> {
        handshake_frame(
            self.clock.now_millis(),
            self.identity.public_key_bundle(),
            &self.identity.signing_key(),
        )
    }

    /// Accepts a handshake and returns a bundle only when trust was established by TOFU.
    pub fn accept_handshake(&mut self, frame: &MessageFrame) -> Result<Option<String>> {
        if frame.payload.is_some() || frame.ephemeral_public_key.is_some() {
            return Err(Error::Crypto);
        }
        let sender = self.verify_server_frame(frame).ok_or(Error::Crypto)?;
        if let Some(trusted) = &self.trusted_server {
            if trusted.as_str() != sender.as_str() {
                return Err(Error::Crypto);
            }
            Ok(None)
        } else {
            let bundle = sender.as_str().to_owned();
            self.trusted_server = Some(sender);
            Ok(Some(bundle))
        }
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<MessageFrame> {
        let server = self.trusted_server.as_ref().ok_or(Error::Crypto)?;
        encrypt_frame(
            self.clock.now_millis(),
            plaintext,
            self.identity.public_key_bundle(),
            &self.identity.signing_key(),
            &server.encryption,
        )
    }

    pub fn decrypt(&mut self, frame: &MessageFrame) -> Result<Option<Zeroizing<Vec<u8>>>> {
        let Some(sender) = self.verify_server_frame(frame) else {
            return Ok(None);
        };
        let Some(trusted) = &self.trusted_server else {
            return Ok(None);
        };
        if sender.as_str() != trusted.as_str()
            || frame.payload.is_none()
            || frame.ephemeral_public_key.is_none()
        {
            return Ok(None);
        }
        let accepted_at = self.clock.monotonic_millis();
        self.nonce_cache.retain(|_, observed_at| {
            accepted_at.saturating_sub(*observed_at) <= FRAME_TIMESTAMP_TOLERANCE_MS as u64
        });
        if self.nonce_cache.contains_key(&frame.nonce)
            || self.nonce_cache.len() >= NONCE_CACHE_CAPACITY
        {
            return Ok(None);
        }
        let Some(plaintext) = decrypt_frame(frame, &self.identity.encryption_key()) else {
            return Ok(None);
        };
        self.nonce_cache.insert(frame.nonce.clone(), accepted_at);
        Ok(Some(Zeroizing::new(plaintext)))
    }

    fn verify_server_frame(&self, frame: &MessageFrame) -> Option<PublicKeyBundle> {
        if frame.version != FRAME_VERSION
            || !valid_nonce(&frame.nonce)
            || frame.timestamp.abs_diff(self.clock.now_millis())
                > FRAME_TIMESTAMP_TOLERANCE_MS as u64
        {
            return None;
        }
        let sender = PublicKeyBundle::parse(&frame.public_key)?;
        verify_frame(frame, &sender).then_some(sender)
    }
}

fn public_key_bundle(signing: &SigningKey, encryption: &PublicKey) -> String {
    format!(
        "v1.{}.{}",
        URL_SAFE_NO_PAD.encode(signing.verifying_key().as_bytes()),
        URL_SAFE_NO_PAD.encode(encryption.as_bytes())
    )
}

fn transcript(frame: &MessageFrame) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        FRAME_TRANSCRIPT_PREFIX,
        frame.timestamp,
        frame.nonce,
        frame.ephemeral_public_key.as_deref().unwrap_or(""),
        frame.public_key,
        frame.payload.as_deref().unwrap_or("")
    )
}

fn header_transcript(frame: &MessageFrame) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        PAYLOAD_HEADER_PREFIX,
        frame.timestamp,
        frame.nonce,
        frame.ephemeral_public_key.as_deref().unwrap_or(""),
        frame.public_key
    )
}

fn verify_frame(frame: &MessageFrame, bundle: &PublicKeyBundle) -> bool {
    let Some(bytes) = decode_canonical(&frame.signature) else {
        return false;
    };
    let Ok(signature) = Signature::from_slice(&bytes) else {
        return false;
    };
    bundle
        .signing
        .verify_strict(transcript(frame).as_bytes(), &signature)
        .is_ok()
}

fn sign_frame(frame: &mut MessageFrame, signing: &SigningKey) {
    frame.signature = URL_SAFE_NO_PAD.encode(signing.sign(transcript(frame).as_bytes()).to_bytes());
}

fn handshake_frame(timestamp: i64, bundle: String, signing: &SigningKey) -> Result<MessageFrame> {
    let mut frame = MessageFrame {
        version: FRAME_VERSION,
        timestamp,
        nonce: URL_SAFE_NO_PAD.encode(random_array::<24>()?),
        ephemeral_public_key: None,
        public_key: bundle,
        payload: None,
        signature: String::new(),
    };
    sign_frame(&mut frame, signing);
    Ok(frame)
}

fn encrypt_frame(
    timestamp: i64,
    plaintext: &[u8],
    sender_bundle: String,
    sender_signing: &SigningKey,
    recipient: &PublicKey,
) -> Result<MessageFrame> {
    let mut secret_bytes = random_array::<32>()?;
    let ephemeral = StaticSecret::from(secret_bytes);
    secret_bytes.zeroize();
    let ephemeral_public = PublicKey::from(&ephemeral);
    let nonce = random_array::<24>()?;
    let mut frame = MessageFrame {
        version: FRAME_VERSION,
        timestamp,
        nonce: URL_SAFE_NO_PAD.encode(nonce),
        ephemeral_public_key: Some(URL_SAFE_NO_PAD.encode(ephemeral_public.as_bytes())),
        public_key: sender_bundle,
        payload: None,
        signature: String::new(),
    };
    let shared = ephemeral.diffie_hellman(recipient);
    if !shared.was_contributory() {
        return Err(Error::Crypto);
    }
    let key = derive_payload_key(shared.as_bytes(), &nonce)?;
    let ciphertext = XChaCha20Poly1305::new((&*key).into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: header_transcript(&frame).as_bytes(),
            },
        )
        .map_err(|_| Error::Crypto)?;
    frame.payload = Some(URL_SAFE_NO_PAD.encode(ciphertext));
    sign_frame(&mut frame, sender_signing);
    if frame_size(&frame) > MAX_FRAME_SIZE {
        return Err(Error::FrameTooLarge);
    }
    Ok(frame)
}

fn decrypt_frame(frame: &MessageFrame, recipient: &StaticSecret) -> Option<Vec<u8>> {
    let nonce: [u8; 24] = decode_canonical(&frame.nonce)?.try_into().ok()?;
    let ephemeral: [u8; 32] = decode_canonical(frame.ephemeral_public_key.as_ref()?)?
        .try_into()
        .ok()?;
    let ciphertext = decode_canonical(frame.payload.as_ref()?)?;
    let shared = recipient.diffie_hellman(&PublicKey::from(ephemeral));
    if !shared.was_contributory() {
        return None;
    }
    let key = derive_payload_key(shared.as_bytes(), &nonce).ok()?;
    XChaCha20Poly1305::new((&*key).into())
        .decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: header_transcript(frame).as_bytes(),
            },
        )
        .ok()
}

fn derive_payload_key(shared: &[u8], nonce: &[u8; 24]) -> Result<Zeroizing<[u8; 32]>> {
    let mut key = Zeroizing::new([0; 32]);
    Hkdf::<Sha256>::new(Some(nonce), shared)
        .expand(PAYLOAD_HKDF_INFO, key.as_mut())
        .map_err(|_| Error::Crypto)?;
    Ok(key)
}

fn frame_size(frame: &MessageFrame) -> usize {
    frame
        .nonce
        .len()
        .saturating_add(frame.ephemeral_public_key.as_ref().map_or(0, String::len))
        .saturating_add(frame.public_key.len())
        .saturating_add(frame.payload.as_ref().map_or(0, String::len))
        .saturating_add(frame.signature.len())
        .saturating_add(128)
}

fn valid_nonce(value: &str) -> bool {
    decode_canonical(value).is_some_and(|bytes| bytes.len() == 24)
}
fn valid_ephemeral_key(value: Option<&str>) -> bool {
    value
        .and_then(decode_canonical)
        .is_some_and(|bytes| bytes.len() == 32)
}
fn decode_canonical(value: &str) -> Option<Vec<u8>> {
    let bytes = URL_SAFE_NO_PAD.decode(value).ok()?;
    (URL_SAFE_NO_PAD.encode(&bytes) == value).then_some(bytes)
}
fn random_array<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::getrandom(&mut bytes).map_err(|_| Error::Crypto)?;
    Ok(bytes)
}
fn decode_32(value: &str) -> Result<Zeroizing<[u8; 32]>> {
    let bytes = Zeroizing::new(
        decode_canonical(value)
            .ok_or_else(|| Error::InvalidState("invalid secret encoding".into()))?,
    );
    Ok(Zeroizing::new(bytes.as_slice().try_into().map_err(
        |_| Error::InvalidState("invalid secret length".into()),
    )?))
}
fn validate_bundles(values: &[String]) -> Result<()> {
    if values
        .iter()
        .all(|value| PublicKeyBundle::parse(value).is_some())
    {
        Ok(())
    } else {
        Err(Error::InvalidState("invalid approved client bundle".into()))
    }
}

#[cfg(test)]
mod tests;
