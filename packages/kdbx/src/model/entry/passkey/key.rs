use minicbor::Encoder;
use p256::ecdsa::{DerSignature, SigningKey as P256SigningKey};
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use rand_core::OsRng;
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, RsaPublicKey};
use zeroize::Zeroizing;

use super::{PasskeyAlgorithm, PasskeyError};

pub(super) enum CredentialKey {
    Es256(p256::SecretKey),
    Rs256(Box<RsaPrivateKey>),
    Ed25519(ed25519_dalek::SigningKey),
}

impl CredentialKey {
    pub(super) fn generate(algorithm: PasskeyAlgorithm) -> Result<Self, PasskeyError> {
        match algorithm {
            PasskeyAlgorithm::Es256 => Ok(Self::Es256(p256::SecretKey::random(&mut OsRng))),
            PasskeyAlgorithm::Rs256 => RsaPrivateKey::new(&mut OsRng, 2048)
                .map(Box::new)
                .map(Self::Rs256)
                .map_err(|_| PasskeyError::KeyGeneration),
            PasskeyAlgorithm::Ed25519 => Ok(Self::Ed25519(ed25519_dalek::SigningKey::generate(
                &mut OsRng,
            ))),
        }
    }

    pub(super) fn from_pkcs8_pem(value: &str) -> Result<Self, PasskeyError> {
        if let Ok(key) = p256::SecretKey::from_pkcs8_pem(value) {
            return Ok(Self::Es256(key));
        }
        if let Ok(key) = RsaPrivateKey::from_pkcs8_pem(value) {
            if key.n().bits() < 2048 {
                return Err(PasskeyError::InvalidPrivateKey);
            }
            return Ok(Self::Rs256(Box::new(key)));
        }
        if let Ok(key) = ed25519_dalek::SigningKey::from_pkcs8_pem(value) {
            return Ok(Self::Ed25519(key));
        }
        Err(PasskeyError::InvalidPrivateKey)
    }

    pub(super) fn algorithm(&self) -> PasskeyAlgorithm {
        match self {
            Self::Es256(_) => PasskeyAlgorithm::Es256,
            Self::Rs256(_) => PasskeyAlgorithm::Rs256,
            Self::Ed25519(_) => PasskeyAlgorithm::Ed25519,
        }
    }

    pub(super) fn to_pkcs8_pem(&self) -> Result<Zeroizing<String>, PasskeyError> {
        match self {
            Self::Es256(key) => key
                .to_pkcs8_pem(LineEnding::LF)
                .map_err(|_| PasskeyError::InvalidPrivateKey),
            Self::Rs256(key) => key
                .to_pkcs8_pem(LineEnding::LF)
                .map_err(|_| PasskeyError::InvalidPrivateKey),
            Self::Ed25519(key) => key
                .to_pkcs8_pem(LineEnding::LF)
                .map_err(|_| PasskeyError::InvalidPrivateKey),
        }
    }

    pub(super) fn to_cose_key(&self) -> Result<Vec<u8>, PasskeyError> {
        let mut encoder = Encoder::new(Vec::new());
        match self {
            Self::Es256(key) => {
                let signing_key = P256SigningKey::from(key.clone());
                let point = signing_key.verifying_key().to_encoded_point(false);
                let x = point.x().ok_or(PasskeyError::PublicKeyEncoding)?;
                let y = point.y().ok_or(PasskeyError::PublicKeyEncoding)?;
                encoder
                    .map(5)
                    .and_then(|encoder| encoder.i32(1))
                    .and_then(|encoder| encoder.i32(2))
                    .and_then(|encoder| encoder.i32(3))
                    .and_then(|encoder| encoder.i32(-7))
                    .and_then(|encoder| encoder.i32(-1))
                    .and_then(|encoder| encoder.i32(1))
                    .and_then(|encoder| encoder.i32(-2))
                    .and_then(|encoder| encoder.bytes(x))
                    .and_then(|encoder| encoder.i32(-3))
                    .and_then(|encoder| encoder.bytes(y))
                    .map_err(|_| PasskeyError::CborEncoding)?;
            }
            Self::Rs256(key) => {
                let public = RsaPublicKey::from(key.as_ref());
                let modulus = public.n().to_bytes_be();
                let exponent = public.e().to_bytes_be();
                encoder
                    .map(4)
                    .and_then(|encoder| encoder.i32(1))
                    .and_then(|encoder| encoder.i32(3))
                    .and_then(|encoder| encoder.i32(3))
                    .and_then(|encoder| encoder.i32(-257))
                    .and_then(|encoder| encoder.i32(-1))
                    .and_then(|encoder| encoder.bytes(&modulus))
                    .and_then(|encoder| encoder.i32(-2))
                    .and_then(|encoder| encoder.bytes(&exponent))
                    .map_err(|_| PasskeyError::CborEncoding)?;
            }
            Self::Ed25519(key) => {
                let public = key.verifying_key().to_bytes();
                encoder
                    .map(4)
                    .and_then(|encoder| encoder.i32(1))
                    .and_then(|encoder| encoder.i32(1))
                    .and_then(|encoder| encoder.i32(3))
                    .and_then(|encoder| encoder.i32(-8))
                    .and_then(|encoder| encoder.i32(-1))
                    .and_then(|encoder| encoder.i32(6))
                    .and_then(|encoder| encoder.i32(-2))
                    .and_then(|encoder| encoder.bytes(&public))
                    .map_err(|_| PasskeyError::CborEncoding)?;
            }
        }
        Ok(encoder.into_writer())
    }

    pub(super) fn public_key_spki(&self) -> Result<Vec<u8>, PasskeyError> {
        match self {
            Self::Es256(key) => key
                .public_key()
                .to_public_key_der()
                .map(|document| document.as_bytes().to_vec())
                .map_err(|_| PasskeyError::PublicKeyEncoding),
            Self::Rs256(key) => RsaPublicKey::from(key.as_ref())
                .to_public_key_der()
                .map(|document| document.as_bytes().to_vec())
                .map_err(|_| PasskeyError::PublicKeyEncoding),
            Self::Ed25519(key) => key
                .verifying_key()
                .to_public_key_der()
                .map(|document| document.as_bytes().to_vec())
                .map_err(|_| PasskeyError::PublicKeyEncoding),
        }
    }

    pub(super) fn sign(&self, message: &[u8]) -> Result<Vec<u8>, PasskeyError> {
        match self {
            Self::Es256(key) => {
                let signing_key = P256SigningKey::from(key.clone());
                let signature: DerSignature =
                    p256::ecdsa::signature::Signer::sign(&signing_key, message);
                Ok(signature.as_bytes().to_vec())
            }
            Self::Rs256(key) => {
                let signing_key =
                    rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new(key.as_ref().clone());
                let signature = rsa::signature::RandomizedSigner::sign_with_rng(
                    &signing_key,
                    &mut OsRng,
                    message,
                );
                Ok(rsa::signature::SignatureEncoding::to_vec(&signature))
            }
            Self::Ed25519(key) => {
                let signature = ed25519_dalek::Signer::sign(key, message);
                Ok(signature.to_bytes().to_vec())
            }
        }
    }
}
