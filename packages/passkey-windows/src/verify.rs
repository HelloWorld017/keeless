//! Strict CNG verification for platform operation and Hello signatures.

use std::{ffi::c_void, ptr};

use sha2::{Digest, Sha256};

use crate::error::NTE_BAD_SIGNATURE;

type NtStatus = i32;
type AlgorithmHandle = *mut c_void;
type KeyHandle = *mut c_void;

const STATUS_SUCCESS: NtStatus = 0;
const BCRYPT_PAD_PKCS1: u32 = 0x2;
const BCRYPT_PAD_PSS: u32 = 0x8;
const RSA_PUBLIC_MAGIC: u32 = 0x3141_5352;
const ECDSA_P256_PUBLIC_MAGIC: u32 = 0x3153_4345;
const ECDSA_P384_PUBLIC_MAGIC: u32 = 0x3353_4345;
const ECDSA_P521_PUBLIC_MAGIC: u32 = 0x3553_4345;
const MAX_RSA_MODULUS_BYTES: usize = 1024;

#[repr(C)]
struct BcryptPssPaddingInfo {
    algorithm: *const u16,
    salt_len: u32,
}

#[link(name = "bcrypt")]
unsafe extern "system" {
    fn BCryptOpenAlgorithmProvider(
        algorithm: *mut AlgorithmHandle,
        algorithm_id: *const u16,
        implementation: *const u16,
        flags: u32,
    ) -> NtStatus;
    fn BCryptCloseAlgorithmProvider(algorithm: AlgorithmHandle, flags: u32) -> NtStatus;
    fn BCryptImportKeyPair(
        algorithm: AlgorithmHandle,
        import_key: KeyHandle,
        blob_type: *const u16,
        key: *mut KeyHandle,
        input: *const u8,
        input_len: u32,
        flags: u32,
    ) -> NtStatus;
    fn BCryptDestroyKey(key: KeyHandle) -> NtStatus;
    fn BCryptVerifySignature(
        key: KeyHandle,
        padding_info: *const c_void,
        hash: *const u8,
        hash_len: u32,
        signature: *const u8,
        signature_len: u32,
        flags: u32,
    ) -> NtStatus;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyKind {
    Rsa,
    EcdsaP256,
    EcdsaP384,
    EcdsaP521,
}

/// Verify a platform signature over the exact encoded request bytes.
pub fn verify_signature(
    public_key: &[u8],
    signed_bytes: &[u8],
    signature: &[u8],
) -> Result<(), i32> {
    if signed_bytes.is_empty() || signature.is_empty() {
        return Err(NTE_BAD_SIGNATURE);
    }
    let kind = validate_public_blob(public_key).ok_or(NTE_BAD_SIGNATURE)?;
    let hash = Sha256::digest(signed_bytes);
    let algorithm_name = match kind {
        KeyKind::Rsa => wide("RSA"),
        KeyKind::EcdsaP256 => wide("ECDSA_P256"),
        KeyKind::EcdsaP384 => wide("ECDSA_P384"),
        KeyKind::EcdsaP521 => wide("ECDSA_P521"),
    };
    let public_blob = wide("PUBLICBLOB");
    let sha256 = wide("SHA256");
    let mut algorithm = ptr::null_mut();
    if unsafe {
        BCryptOpenAlgorithmProvider(&mut algorithm, algorithm_name.as_ptr(), ptr::null(), 0)
    } != STATUS_SUCCESS
    {
        return Err(NTE_BAD_SIGNATURE);
    }
    let mut key = ptr::null_mut();
    let result = (|| {
        if unsafe {
            BCryptImportKeyPair(
                algorithm,
                ptr::null_mut(),
                public_blob.as_ptr(),
                &mut key,
                public_key.as_ptr(),
                public_key.len().try_into().map_err(|_| NTE_BAD_SIGNATURE)?,
                0,
            )
        } != STATUS_SUCCESS
        {
            return Err(NTE_BAD_SIGNATURE);
        }

        let verified = match kind {
            KeyKind::Rsa => {
                let padding = BcryptPssPaddingInfo {
                    algorithm: sha256.as_ptr(),
                    salt_len: hash.len() as u32,
                };
                (unsafe {
                    BCryptVerifySignature(
                        key,
                        (&raw const padding).cast(),
                        hash.as_ptr(),
                        hash.len() as u32,
                        signature.as_ptr(),
                        signature.len().try_into().map_err(|_| NTE_BAD_SIGNATURE)?,
                        BCRYPT_PAD_PSS,
                    )
                }) == STATUS_SUCCESS
                    || (unsafe {
                        BCryptVerifySignature(
                            key,
                            ptr::null(),
                            hash.as_ptr(),
                            hash.len() as u32,
                            signature.as_ptr(),
                            signature.len().try_into().map_err(|_| NTE_BAD_SIGNATURE)?,
                            BCRYPT_PAD_PKCS1,
                        )
                    }) == STATUS_SUCCESS
            }
            KeyKind::EcdsaP256 | KeyKind::EcdsaP384 | KeyKind::EcdsaP521 => {
                (unsafe {
                    BCryptVerifySignature(
                        key,
                        ptr::null(),
                        hash.as_ptr(),
                        hash.len() as u32,
                        signature.as_ptr(),
                        signature.len().try_into().map_err(|_| NTE_BAD_SIGNATURE)?,
                        0,
                    )
                }) == STATUS_SUCCESS
            }
        };
        verified.then_some(()).ok_or(NTE_BAD_SIGNATURE)
    })();
    if !key.is_null() {
        unsafe { BCryptDestroyKey(key) };
    }
    unsafe { BCryptCloseAlgorithmProvider(algorithm, 0) };
    result
}

fn validate_public_blob(blob: &[u8]) -> Option<KeyKind> {
    let magic = read_u32(blob, 0)?;
    match magic {
        RSA_PUBLIC_MAGIC => {
            if blob.len() < 24 {
                return None;
            }
            let bits = read_u32(blob, 4)? as usize;
            let exponent_len = read_u32(blob, 8)? as usize;
            let modulus_len = read_u32(blob, 12)? as usize;
            if read_u32(blob, 16)? != 0
                || read_u32(blob, 20)? != 0
                || exponent_len == 0
                || exponent_len > 8
                || modulus_len == 0
                || modulus_len > MAX_RSA_MODULUS_BYTES
                || bits != modulus_len.checked_mul(8)?
                || blob.len()
                    != 24usize
                        .checked_add(exponent_len)?
                        .checked_add(modulus_len)?
            {
                return None;
            }
            Some(KeyKind::Rsa)
        }
        ECDSA_P256_PUBLIC_MAGIC => validate_ecc_blob(blob, 32).then_some(KeyKind::EcdsaP256),
        ECDSA_P384_PUBLIC_MAGIC => validate_ecc_blob(blob, 48).then_some(KeyKind::EcdsaP384),
        ECDSA_P521_PUBLIC_MAGIC => validate_ecc_blob(blob, 66).then_some(KeyKind::EcdsaP521),
        _ => None,
    }
}

fn validate_ecc_blob(blob: &[u8], expected_coordinate_len: usize) -> bool {
    blob.len() == 8 + 2 * expected_coordinate_len
        && read_u32(blob, 4) == Some(expected_coordinate_len as u32)
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes
        .get(offset..offset.checked_add(4)?)?
        .try_into()
        .ok()
        .map(u32::from_le_bytes)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_malformed_or_unknown_public_blobs() {
        assert!(validate_public_blob(&[]).is_none());
        assert!(validate_public_blob(&[0; 24]).is_none());
        let mut ecc = Vec::new();
        ecc.extend(ECDSA_P256_PUBLIC_MAGIC.to_le_bytes());
        ecc.extend(32_u32.to_le_bytes());
        ecc.resize(72, 1);
        assert_eq!(validate_public_blob(&ecc), Some(KeyKind::EcdsaP256));
        ecc.pop();
        assert!(validate_public_blob(&ecc).is_none());
    }
}
