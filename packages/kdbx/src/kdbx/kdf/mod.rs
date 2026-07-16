//! KDF (Key Derivation Function) engines

pub mod aes_kdf;
pub mod argon2_kdf;
pub mod kdf_engine;
pub mod kdf_parameters;

pub use kdf_engine::KdfEngine;
pub use kdf_parameters::KdfParameters;

use uuid::Uuid;
use aes_kdf::{AesKdf, AES_KDF_UUID};
use argon2_kdf::{Argon2Kdf, ARGON2D_UUID, ARGON2ID_UUID};

/// Create a KDF engine from its UUID.
pub fn create_kdf(uuid: &Uuid) -> Option<Box<dyn KdfEngine>> {
    match *uuid {
        AES_KDF_UUID => Some(Box::new(AesKdf)),
        ARGON2D_UUID => Some(Box::new(Argon2Kdf::argon2d())),
        ARGON2ID_UUID => Some(Box::new(Argon2Kdf::argon2id())),
        _ => None,
    }
}
