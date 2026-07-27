//! CTAP2 status codes.
//!
//! Every authenticator response carries one of these as its first byte, so both
//! transports map their failures onto this type rather than inventing their own.

/// A CTAP2 status byte, as defined by the CTAP 2.1 error-response table.
///
/// The values are the wire encoding, so they must match the specification's
/// table exactly: a platform reads them to decide whether to retry, to prompt,
/// or to move on to another authenticator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CtapStatus {
    Success = 0x00,
    InvalidCommand = 0x01,
    InvalidParameter = 0x02,
    InvalidLength = 0x03,
    CborUnexpectedType = 0x11,
    InvalidCbor = 0x12,
    MissingParameter = 0x14,
    CredentialExcluded = 0x19,
    UnsupportedAlgorithm = 0x26,
    OperationDenied = 0x27,
    KeyStoreFull = 0x28,
    UnsupportedOption = 0x2b,
    InvalidOption = 0x2c,
    KeepaliveCancel = 0x2d,
    NoCredentials = 0x2e,
    UserActionTimeout = 0x2f,
    NotAllowed = 0x30,
    PinAuthInvalid = 0x33,
    RequestTooLarge = 0x39,
    ActionTimeout = 0x3a,
    UserVerificationInvalid = 0x3f,
    Other = 0x7f,
}

impl CtapStatus {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// A CTAP2 command that failed, carrying the status to answer with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("CTAP error {:#04x}", self.status.as_u8())]
pub struct CtapError {
    pub status: CtapStatus,
}

impl CtapError {
    pub const fn new(status: CtapStatus) -> Self {
        Self { status }
    }
}

impl From<CtapStatus> for CtapError {
    fn from(status: CtapStatus) -> Self {
        Self::new(status)
    }
}

pub type Result<T> = core::result::Result<T, CtapError>;
