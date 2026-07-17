use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use keeless_schema::MessageFrame;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize, Zeroizing};

use crate::{CoreError, Result, random_array};

pub const FRAME_VERSION: u8 = 1;
pub const FRAME_TIMESTAMP_TOLERANCE_MS: i64 = 500;
pub const NONCE_CACHE_CAPACITY: usize = 2048;
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;
pub const MAX_REQUEST_SIZE: usize = 256 * 1024;
pub const MAX_REQUEST_ID_LENGTH: usize = 128;
pub const FRAME_TRANSCRIPT_PREFIX: &str = "keeless-frame-v1";
pub const PAYLOAD_HEADER_PREFIX: &str = "keeless-payload-header-v1";
pub const PAYLOAD_HKDF_INFO: &[u8] = b"keeless-payload-v1";

pub(crate) struct PublicKeyBundle {
    pub signing: VerifyingKey,
    pub encryption: PublicKey,
}

pub(crate) fn parse_public_key_bundle(value: &str) -> Option<PublicKeyBundle> {
    let mut parts = value.split('.');
    if parts.next()? != "v1" {
        return None;
    }
    let signing: [u8; 32] = decode_canonical(parts.next()?)?.try_into().ok()?;
    let encryption: [u8; 32] = decode_canonical(parts.next()?)?.try_into().ok()?;
    if parts.next().is_some() {
        return None;
    }
    if encryption == [0; 32] {
        return None;
    }
    let signing = VerifyingKey::from_bytes(&signing).ok()?;
    if signing.is_weak() {
        return None;
    }
    Some(PublicKeyBundle {
        signing,
        encryption: PublicKey::from(encryption),
    })
}

pub(crate) fn public_key_bundle(signing: &SigningKey, encryption: &PublicKey) -> String {
    format!(
        "v1.{}.{}",
        URL_SAFE_NO_PAD.encode(signing.verifying_key().as_bytes()),
        URL_SAFE_NO_PAD.encode(encryption.as_bytes())
    )
}

pub(crate) fn transcript(frame: &MessageFrame) -> String {
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

pub(crate) fn verify_frame(frame: &MessageFrame, bundle: &PublicKeyBundle) -> bool {
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

pub(crate) fn sign_frame(frame: &mut MessageFrame, signing: &SigningKey) {
    frame.signature = URL_SAFE_NO_PAD.encode(signing.sign(transcript(frame).as_bytes()).to_bytes());
}

pub(crate) fn handshake_frame(
    timestamp: i64,
    bundle: String,
    signing: &SigningKey,
) -> Result<MessageFrame> {
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

pub(crate) fn encrypt_frame(
    timestamp: i64,
    plaintext: &[u8],
    sender_bundle: String,
    sender_signing: &SigningKey,
    recipient: &PublicKey,
) -> Result<MessageFrame> {
    let ephemeral = random_static_secret()?;
    let ephemeral_public = PublicKey::from(&ephemeral);
    let nonce = random_array::<24>()?;
    let nonce_string = URL_SAFE_NO_PAD.encode(nonce);
    let mut frame = MessageFrame {
        version: FRAME_VERSION,
        timestamp,
        nonce: nonce_string,
        ephemeral_public_key: Some(URL_SAFE_NO_PAD.encode(ephemeral_public.as_bytes())),
        public_key: sender_bundle,
        payload: None,
        signature: String::new(),
    };
    let shared = ephemeral.diffie_hellman(recipient);
    if !shared.was_contributory() {
        return Err(CoreError::Crypto);
    }
    let key = derive_payload_key(shared.as_bytes(), &nonce)?;
    let aad = header_transcript(&frame);
    let ciphertext = XChaCha20Poly1305::new((&*key).into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| CoreError::Crypto)?;
    frame.payload = Some(URL_SAFE_NO_PAD.encode(ciphertext));
    sign_frame(&mut frame, sender_signing);
    Ok(frame)
}

pub(crate) fn decrypt_frame(frame: &MessageFrame, recipient: &StaticSecret) -> Option<Vec<u8>> {
    let nonce: [u8; 24] = URL_SAFE_NO_PAD.decode(&frame.nonce).ok()?.try_into().ok()?;
    let ephemeral: [u8; 32] = URL_SAFE_NO_PAD
        .decode(frame.ephemeral_public_key.as_ref()?)
        .ok()?
        .try_into()
        .ok()?;
    let ciphertext = URL_SAFE_NO_PAD.decode(frame.payload.as_ref()?).ok()?;
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
        .map_err(|_| CoreError::Crypto)?;
    Ok(key)
}

fn random_static_secret() -> Result<StaticSecret> {
    let mut bytes = random_array::<32>()?;
    let secret = StaticSecret::from(bytes);
    bytes.zeroize();
    Ok(secret)
}

pub(crate) fn valid_nonce(value: &str) -> bool {
    decode_canonical(value)
        .map(|bytes| bytes.len() == 24)
        .unwrap_or(false)
}

pub(crate) fn valid_ephemeral_key(value: Option<&str>) -> bool {
    value
        .and_then(decode_canonical)
        .map(|bytes| bytes.len() == 32)
        .unwrap_or(false)
}

fn decode_canonical(value: &str) -> Option<Vec<u8>> {
    let bytes = URL_SAFE_NO_PAD.decode(value).ok()?;
    (URL_SAFE_NO_PAD.encode(&bytes) == value).then_some(bytes)
}
