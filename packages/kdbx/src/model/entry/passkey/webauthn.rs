use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use minicbor::Encoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::{Host, Url};

use super::{PasskeyError, UserVerification};

/// Stable AAGUID assigned to the Keeless software authenticator.
pub const KEELESS_AAGUID: [u8; 16] = [
    0x89, 0xec, 0x85, 0x72, 0xca, 0xec, 0x48, 0xc2, 0xa5, 0x29, 0xeb, 0x4a, 0x87, 0xe1, 0xbf, 0xf0,
];

const FLAG_UP: u8 = 0x01;
const FLAG_UV: u8 = 0x04;
const FLAG_BE: u8 = 0x08;
const FLAG_BS: u8 = 0x10;
const FLAG_AT: u8 = 0x40;

#[derive(Clone, Copy)]
pub(super) enum Ceremony {
    Create,
    Get,
}

impl Ceremony {
    fn client_data_type(self) -> &'static str {
        match self {
            Self::Create => "webauthn.create",
            Self::Get => "webauthn.get",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CollectedClientData {
    #[serde(rename = "type")]
    ceremony_type: String,
    challenge: String,
    origin: String,
    #[serde(default)]
    cross_origin: bool,
    #[serde(default)]
    top_origin: Option<String>,
}

pub(super) fn validate_client_data(
    client_data_json: &[u8],
    trusted_origin: &str,
    expected_challenge: &[u8],
    rp_id: &str,
    ceremony: Ceremony,
) -> Result<String, PasskeyError> {
    if expected_challenge.len() < 16 {
        return Err(PasskeyError::ChallengeTooShort);
    }
    let client_data: CollectedClientData =
        serde_json::from_slice(client_data_json).map_err(|_| PasskeyError::InvalidClientData)?;
    if client_data.ceremony_type != ceremony.client_data_type() {
        return Err(PasskeyError::InvalidCeremonyType);
    }
    if client_data.cross_origin || client_data.top_origin.is_some() {
        return Err(PasskeyError::CrossOriginNotSupported);
    }
    let challenge = URL_SAFE_NO_PAD
        .decode(&client_data.challenge)
        .map_err(|_| PasskeyError::InvalidClientData)?;
    if challenge != expected_challenge {
        return Err(PasskeyError::ChallengeMismatch);
    }

    let origin = parse_origin(&client_data.origin)?;
    let trusted_origin = parse_origin(trusted_origin)?;
    if origin.origin() != trusted_origin.origin() {
        return Err(PasskeyError::OriginMismatch);
    }
    let normalized_rp_id = normalize_rp_id(rp_id)?;
    validate_rp_for_origin(&normalized_rp_id, &trusted_origin)?;
    Ok(normalized_rp_id)
}

pub(super) fn normalize_stored_rp_id(rp_id: &str) -> Result<String, PasskeyError> {
    normalize_rp_id(rp_id)
}

pub(super) fn registration_authenticator_data(
    rp_id: &str,
    user_verification: UserVerification,
    backup_eligible: bool,
    backup_state: bool,
    credential_id: &[u8],
    public_key_cose: &[u8],
) -> Result<Vec<u8>, PasskeyError> {
    if credential_id.len() > u16::MAX as usize || (backup_state && !backup_eligible) {
        return Err(PasskeyError::InvalidBackupFlags);
    }
    let mut result = Vec::with_capacity(55 + credential_id.len() + public_key_cose.len());
    result.extend_from_slice(&Sha256::digest(rp_id.as_bytes()));
    result.push(flags(
        user_verification,
        backup_eligible,
        backup_state,
        true,
    ));
    result.extend_from_slice(&0u32.to_be_bytes());
    result.extend_from_slice(&KEELESS_AAGUID);
    result.extend_from_slice(&(credential_id.len() as u16).to_be_bytes());
    result.extend_from_slice(credential_id);
    result.extend_from_slice(public_key_cose);
    Ok(result)
}

pub(super) fn assertion_authenticator_data(
    rp_id: &str,
    user_verification: UserVerification,
    backup_eligible: bool,
    backup_state: bool,
) -> Vec<u8> {
    let mut result = Vec::with_capacity(37);
    result.extend_from_slice(&Sha256::digest(rp_id.as_bytes()));
    result.push(flags(
        user_verification,
        backup_eligible,
        backup_state,
        false,
    ));
    result.extend_from_slice(&0u32.to_be_bytes());
    result
}

pub(super) fn assertion_signature_message(
    authenticator_data: &[u8],
    client_data_json: &[u8],
) -> Vec<u8> {
    let mut result = Vec::with_capacity(authenticator_data.len() + 32);
    result.extend_from_slice(authenticator_data);
    result.extend_from_slice(&Sha256::digest(client_data_json));
    result
}

pub(super) fn none_attestation(authenticator_data: &[u8]) -> Result<Vec<u8>, PasskeyError> {
    let mut encoder = Encoder::new(Vec::new());
    encoder
        .map(3)
        .and_then(|encoder| encoder.str("fmt"))
        .and_then(|encoder| encoder.str("none"))
        .and_then(|encoder| encoder.str("attStmt"))
        .and_then(|encoder| encoder.map(0))
        .and_then(|encoder| encoder.str("authData"))
        .and_then(|encoder| encoder.bytes(authenticator_data))
        .map_err(|_| PasskeyError::CborEncoding)?;
    Ok(encoder.into_writer())
}

fn flags(
    user_verification: UserVerification,
    backup_eligible: bool,
    backup_state: bool,
    attested_credential_data: bool,
) -> u8 {
    let mut result = FLAG_UP;
    if user_verification == UserVerification::Verified {
        result |= FLAG_UV;
    }
    if backup_eligible {
        result |= FLAG_BE;
    }
    if backup_state {
        result |= FLAG_BS;
    }
    if attested_credential_data {
        result |= FLAG_AT;
    }
    result
}

fn parse_origin(origin: &str) -> Result<Url, PasskeyError> {
    let url = Url::parse(origin).map_err(|_| PasskeyError::InvalidOrigin)?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(PasskeyError::InvalidOrigin);
    }
    let host = url.host().ok_or(PasskeyError::InvalidOrigin)?;
    let secure = url.scheme() == "https";
    let local_http = url.scheme() == "http" && is_local_host(&host);
    if !secure && !local_http {
        return Err(PasskeyError::InvalidOrigin);
    }
    Ok(url)
}

fn normalize_rp_id(rp_id: &str) -> Result<String, PasskeyError> {
    if rp_id.is_empty()
        || rp_id.ends_with('.')
        || rp_id.contains(['/', '\\', ':', '@', '?', '#'])
        || rp_id.chars().any(char::is_whitespace)
    {
        return Err(PasskeyError::InvalidRpId);
    }
    match Host::parse(rp_id).map_err(|_| PasskeyError::InvalidRpId)? {
        Host::Domain(host) if !host.is_empty() => Ok(host.to_ascii_lowercase()),
        Host::Domain(_) | Host::Ipv4(_) | Host::Ipv6(_) => Err(PasskeyError::InvalidRpId),
    }
}

fn validate_rp_for_origin(rp_id: &str, origin: &Url) -> Result<(), PasskeyError> {
    match origin.host().ok_or(PasskeyError::InvalidOrigin)? {
        Host::Domain(host) => {
            let host = host.to_ascii_lowercase();
            let related = host == rp_id
                || host
                    .strip_suffix(rp_id)
                    .is_some_and(|prefix| prefix.ends_with('.'));
            if !related {
                return Err(PasskeyError::RpIdOriginMismatch);
            }
            if rp_id == "localhost" || rp_id.ends_with(".localhost") {
                return Ok(());
            }
            if psl::domain(rp_id.as_bytes()).is_none() {
                return Err(PasskeyError::InvalidRpId);
            }
        }
        Host::Ipv4(_) | Host::Ipv6(_) => return Err(PasskeyError::InvalidRpId),
    }
    Ok(())
}

fn is_local_host(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(value) => *value == "localhost" || value.ends_with(".localhost"),
        Host::Ipv4(value) => value.is_loopback(),
        Host::Ipv6(value) => value.is_loopback(),
    }
}
