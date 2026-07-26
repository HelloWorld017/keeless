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

#[cfg(test)]
mod tests;

pub use error::{CtapError, CtapStatus, Result};
pub use request::{Command, GetAssertionRequest, MakeCredentialRequest, parse_command};
pub use response::{Assertion, AuthenticatorInfo};
