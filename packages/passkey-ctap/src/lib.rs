//! CTAP2 command codec for the Keeless authenticator.
//!
//! Both authenticator transports speak CTAP2 CBOR — the virtual HID daemon on
//! Linux carries it over CTAPHID, and the Windows plugin authenticator receives
//! it from the platform's WebAuthn service — so command parsing, response
//! encoding, and the status codes live here once.
//!
//! This crate performs no I/O and holds no credentials: it turns bytes into
//! requests and results back into bytes.

pub mod error;
pub mod request;
pub mod response;

/// AAGUID of the Keeless software authenticator.
///
/// Must equal `keeless_kdbx::KEELESS_AAGUID`, which stamps it into attested
/// credential data. A mismatch would have the authenticator describe itself as a
/// different model than the credentials it issues; `keeless_passkey_linux` tests for it.
pub const KEELESS_AAGUID: [u8; 16] = [
    0x89, 0xec, 0x85, 0x72, 0xca, 0xec, 0x48, 0xc2, 0xa5, 0x29, 0xeb, 0x4a, 0x87, 0xe1, 0xbf, 0xf0,
];

#[cfg(test)]
mod tests;

pub use error::{CtapError, CtapStatus, Result};
pub use request::{Command, GetAssertionRequest, MakeCredentialRequest, parse_command};
pub use response::{Assertion, AuthenticatorInfo};
