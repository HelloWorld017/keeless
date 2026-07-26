//! Encoding of CTAP2 authenticator responses.
//!
//! Every payload starts with a status byte; a successful one is followed by the
//! command's CBOR map. Callers hand the result straight to their transport.

use minicbor::Encoder;

use crate::error::{CtapError, CtapStatus, Result};

/// COSE algorithms this authenticator can create credentials with, most preferred first.
pub const SUPPORTED_ALGORITHMS: [i32; 3] = [-7, -8, -257];

/// Largest CTAP message this authenticator accepts, in bytes.
pub const MAX_MESSAGE_SIZE: u32 = 7609;

/// Largest `excludeList` or `allowList` this authenticator will read.
pub const MAX_CREDENTIAL_COUNT_IN_LIST: u32 = 16;

/// Length of the credential IDs this authenticator issues.
pub const MAX_CREDENTIAL_ID_LENGTH: u32 = 32;

/// How the authenticator presents itself in `authenticatorGetInfo`.
pub struct AuthenticatorInfo<'a> {
    pub aaguid: &'a [u8; 16],
    /// Whether the authenticator is built into the platform. A virtual HID device
    /// is presented as a removable key; a platform plugin is not.
    pub platform_device: bool,
    /// Transports to advertise, such as `usb` or `internal`.
    pub transports: &'a [&'a str],
}

/// A credential returned by `authenticatorGetAssertion`.
pub struct Assertion<'a> {
    pub credential_id: &'a [u8],
    pub authenticator_data: &'a [u8],
    pub signature: &'a [u8],
    pub user_id: &'a [u8],
    pub user_name: &'a str,
}

/// A single status byte, for errors and for commands with no response data.
pub fn status(status: CtapStatus) -> Vec<u8> {
    vec![status.as_u8()]
}

/// `authenticatorMakeCredential` response with the `none` attestation statement.
pub fn make_credential(authenticator_data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = Encoder::new(success_payload());
    encoder
        .map(3)
        .and_then(|encoder| encoder.u8(0x01))
        .and_then(|encoder| encoder.str("none"))
        .and_then(|encoder| encoder.u8(0x02))
        .and_then(|encoder| encoder.bytes(authenticator_data))
        .and_then(|encoder| encoder.u8(0x03))
        .and_then(|encoder| encoder.map(0))
        .map_err(encoding_error)?;
    Ok(encoder.into_writer())
}

/// `authenticatorGetAssertion` response.
///
/// The user entity is always included: these are discoverable credentials, and
/// a platform needs the handle to complete a username-less sign-in.
pub fn get_assertion(assertion: &Assertion<'_>) -> Result<Vec<u8>> {
    let mut encoder = Encoder::new(success_payload());
    encoder
        .map(4)
        .and_then(|encoder| encoder.u8(0x01))
        .and_then(|encoder| encoder.map(2))
        .and_then(|encoder| encoder.str("id"))
        .and_then(|encoder| encoder.bytes(assertion.credential_id))
        .and_then(|encoder| encoder.str("type"))
        .and_then(|encoder| encoder.str("public-key"))
        .and_then(|encoder| encoder.u8(0x02))
        .and_then(|encoder| encoder.bytes(assertion.authenticator_data))
        .and_then(|encoder| encoder.u8(0x03))
        .and_then(|encoder| encoder.bytes(assertion.signature))
        .and_then(|encoder| encoder.u8(0x04))
        .and_then(|encoder| encoder.map(2))
        .and_then(|encoder| encoder.str("id"))
        .and_then(|encoder| encoder.bytes(assertion.user_id))
        .and_then(|encoder| encoder.str("name"))
        .and_then(|encoder| encoder.str(assertion.user_name))
        .map_err(encoding_error)?;
    Ok(encoder.into_writer())
}

/// `authenticatorGetInfo` response.
///
/// Declares resident keys, user presence and built-in user verification, and no
/// `clientPin` entry at all — the key is omitted rather than set false, which is
/// how a platform learns the authenticator has no PIN to set.
pub fn get_info(info: &AuthenticatorInfo<'_>) -> Result<Vec<u8>> {
    let mut encoder = Encoder::new(success_payload());
    encoder
        .map(8)
        .and_then(|encoder| encoder.u8(0x01))
        .and_then(|encoder| encoder.array(2))
        .and_then(|encoder| encoder.str("FIDO_2_0"))
        .and_then(|encoder| encoder.str("FIDO_2_1"))
        .and_then(|encoder| encoder.u8(0x03))
        .and_then(|encoder| encoder.bytes(info.aaguid))
        .and_then(|encoder| encoder.u8(0x04))
        .and_then(|encoder| encoder.map(4))
        .and_then(|encoder| encoder.str("rk"))
        .and_then(|encoder| encoder.bool(true))
        .and_then(|encoder| encoder.str("up"))
        .and_then(|encoder| encoder.bool(true))
        .and_then(|encoder| encoder.str("uv"))
        .and_then(|encoder| encoder.bool(true))
        .and_then(|encoder| encoder.str("plat"))
        .and_then(|encoder| encoder.bool(info.platform_device))
        .and_then(|encoder| encoder.u8(0x05))
        .and_then(|encoder| encoder.u32(MAX_MESSAGE_SIZE))
        .and_then(|encoder| encoder.u8(0x07))
        .and_then(|encoder| encoder.u32(MAX_CREDENTIAL_COUNT_IN_LIST))
        .and_then(|encoder| encoder.u8(0x08))
        .and_then(|encoder| encoder.u32(MAX_CREDENTIAL_ID_LENGTH))
        .map_err(encoding_error)?;

    encoder.u8(0x09).map_err(encoding_error)?;
    encoder
        .array(info.transports.len() as u64)
        .map_err(encoding_error)?;
    for transport in info.transports {
        encoder.str(transport).map_err(encoding_error)?;
    }

    encoder.u8(0x0a).map_err(encoding_error)?;
    encoder
        .array(SUPPORTED_ALGORITHMS.len() as u64)
        .map_err(encoding_error)?;
    for algorithm in SUPPORTED_ALGORITHMS {
        encoder
            .map(2)
            .and_then(|encoder| encoder.str("alg"))
            .and_then(|encoder| encoder.i32(algorithm))
            .and_then(|encoder| encoder.str("type"))
            .and_then(|encoder| encoder.str("public-key"))
            .map_err(encoding_error)?;
    }

    Ok(encoder.into_writer())
}

fn success_payload() -> Vec<u8> {
    vec![CtapStatus::Success.as_u8()]
}

fn encoding_error<E>(_: E) -> CtapError {
    CtapError::new(CtapStatus::Other)
}
