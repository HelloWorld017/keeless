//! Decoding of CTAP2 authenticator commands.

use minicbor::Decoder;

use crate::error::{CtapError, CtapStatus, Result};

/// CTAP2 command bytes that prefix an `authenticatorRequest` payload.
mod command {
    pub const MAKE_CREDENTIAL: u8 = 0x01;
    pub const GET_ASSERTION: u8 = 0x02;
    pub const GET_INFO: u8 = 0x04;
    pub const CLIENT_PIN: u8 = 0x06;
    pub const RESET: u8 = 0x07;
    pub const GET_NEXT_ASSERTION: u8 = 0x08;
    pub const SELECTION: u8 = 0x0b;
}

/// Shown for a credential whose relying party sent no user name at all, which
/// CTAP 2.1 permits but the entry still needs something to display.
const UNNAMED_USER: &str = "unknown";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    MakeCredential(Box<MakeCredentialRequest>),
    GetAssertion(GetAssertionRequest),
    GetNextAssertion,
    GetInfo,
    /// A bare presence check used by platforms to pick between authenticators.
    Selection,
    Reset,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct MakeCredentialRequest {
    pub client_data_hash: Vec<u8>,
    pub rp_id: String,
    pub rp_name: Option<String>,
    pub user_id: Vec<u8>,
    pub user_name: String,
    pub user_display_name: Option<String>,
    /// COSE algorithm identifiers in relying-party preference order.
    pub algorithms: Vec<i32>,
    pub exclude_credential_ids: Vec<Vec<u8>>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct GetAssertionRequest {
    pub rp_id: String,
    pub client_data_hash: Vec<u8>,
    pub allow_credential_ids: Vec<Vec<u8>>,
    /// False when the platform asked for a silent assertion.
    ///
    /// Platforms use this to find out which of a relying party's credentials
    /// this authenticator holds before deciding whether to prompt at all, so
    /// refusing it would make every such sign-in look like it had no credentials.
    pub user_presence: bool,
}

impl Default for GetAssertionRequest {
    fn default() -> Self {
        Self {
            rp_id: String::new(),
            client_data_hash: Vec::new(),
            allow_credential_ids: Vec::new(),
            user_presence: true,
        }
    }
}

/// Parse a CTAPHID CBOR payload: one command byte followed by its parameter map.
///
/// Beyond structure this rejects what the authenticator cannot honour at all —
/// a PIN protocol it does not implement, or a request to skip user presence —
/// so both transports refuse them identically.
pub fn parse_command(payload: &[u8]) -> Result<Command> {
    let (command, parameters) = payload
        .split_first()
        .ok_or(CtapError::new(CtapStatus::InvalidLength))?;
    match *command {
        command::MAKE_CREDENTIAL => parse_make_credential(parameters)
            .map(|request| Command::MakeCredential(Box::new(request))),
        command::GET_ASSERTION => parse_get_assertion(parameters).map(Command::GetAssertion),
        command::GET_NEXT_ASSERTION => Ok(Command::GetNextAssertion),
        command::GET_INFO => Ok(Command::GetInfo),
        command::SELECTION => Ok(Command::Selection),
        command::RESET => Ok(Command::Reset),
        // A PIN-capable platform must be told this authenticator has no PIN,
        // which `authenticatorGetInfo` already says, so treat it as unknown.
        command::CLIENT_PIN => Err(CtapStatus::InvalidCommand.into()),
        _ => Err(CtapStatus::InvalidCommand.into()),
    }
}

fn parse_make_credential(parameters: &[u8]) -> Result<MakeCredentialRequest> {
    let mut decoder = Decoder::new(parameters);
    let mut request = MakeCredentialRequest::default();
    let mut seen_hash = false;
    let mut seen_rp = false;
    let mut seen_user = false;
    let mut seen_algorithms = false;
    let mut pin_auth = PinAuth::default();

    for _ in 0..map_length(&mut decoder)? {
        match decoder.u32().map_err(cbor_error)? {
            0x01 => {
                request.client_data_hash = decoder.bytes().map_err(cbor_error)?.to_vec();
                seen_hash = true;
            }
            0x02 => {
                let entity = parse_entity(&mut decoder, EntityId::Text)?;
                request.rp_id = entity.text_id;
                request.rp_name = entity.name;
                seen_rp = true;
            }
            0x03 => {
                let entity = parse_entity(&mut decoder, EntityId::Bytes)?;
                let display_name = entity.display_name.filter(|value| !value.is_empty());
                request.user_id = entity.binary_id;
                request.user_name = entity
                    .name
                    .filter(|value| !value.is_empty())
                    .or_else(|| display_name.clone())
                    .unwrap_or_else(|| UNNAMED_USER.to_string());
                request.user_display_name = display_name;
                seen_user = true;
            }
            0x04 => {
                request.algorithms = parse_credential_parameters(&mut decoder)?;
                seen_algorithms = true;
            }
            0x05 => request.exclude_credential_ids = parse_credential_list(&mut decoder)?,
            0x07 => {
                // Creating a credential the user never approved is not something
                // this authenticator offers, whatever the platform asks for.
                if parse_options(&mut decoder)?.user_presence == Some(false) {
                    return Err(CtapStatus::InvalidOption.into());
                }
            }
            0x08 => {
                pin_auth.token = true;
                decoder.skip().map_err(cbor_error)?;
            }
            0x09 => {
                pin_auth.protocol = true;
                decoder.skip().map_err(cbor_error)?;
            }
            _ => decoder.skip().map_err(cbor_error)?,
        }
    }

    pin_auth.reject()?;
    if !(seen_hash && seen_rp && seen_user && seen_algorithms) {
        return Err(CtapStatus::MissingParameter.into());
    }
    Ok(request)
}

fn parse_get_assertion(parameters: &[u8]) -> Result<GetAssertionRequest> {
    let mut decoder = Decoder::new(parameters);
    let mut request = GetAssertionRequest::default();
    let mut seen_rp = false;
    let mut seen_hash = false;
    let mut pin_auth = PinAuth::default();

    for _ in 0..map_length(&mut decoder)? {
        match decoder.u32().map_err(cbor_error)? {
            0x01 => {
                request.rp_id = decoder.str().map_err(cbor_error)?.to_string();
                seen_rp = true;
            }
            0x02 => {
                request.client_data_hash = decoder.bytes().map_err(cbor_error)?.to_vec();
                seen_hash = true;
            }
            0x03 => request.allow_credential_ids = parse_credential_list(&mut decoder)?,
            0x05 => {
                request.user_presence = parse_options(&mut decoder)?.user_presence.unwrap_or(true);
            }
            0x06 => {
                pin_auth.token = true;
                decoder.skip().map_err(cbor_error)?;
            }
            0x07 => {
                pin_auth.protocol = true;
                decoder.skip().map_err(cbor_error)?;
            }
            _ => decoder.skip().map_err(cbor_error)?,
        }
    }

    pin_auth.reject()?;
    if !(seen_rp && seen_hash) {
        return Err(CtapStatus::MissingParameter.into());
    }
    Ok(request)
}

/// Whether the request carried PIN/UV authentication this authenticator has none of.
#[derive(Default)]
struct PinAuth {
    token: bool,
    protocol: bool,
}

impl PinAuth {
    /// Refuse a PIN-authenticated request with the status its shape calls for.
    ///
    /// A token names a protocol version, and every version is unsupported here,
    /// so a token with a version is an invalid parameter while a token without
    /// one is simply incomplete.
    fn reject(&self) -> Result<()> {
        match (self.token, self.protocol) {
            (true, true) => Err(CtapStatus::InvalidParameter.into()),
            (true, false) => Err(CtapStatus::MissingParameter.into()),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy)]
enum EntityId {
    Text,
    Bytes,
}

#[derive(Default)]
struct Entity {
    text_id: String,
    binary_id: Vec<u8>,
    name: Option<String>,
    display_name: Option<String>,
}

/// Read a relying-party or user entity. Which `id` type is expected differs:
/// a relying party is identified by text, a user by bytes.
fn parse_entity(decoder: &mut Decoder<'_>, id: EntityId) -> Result<Entity> {
    let mut entity = Entity::default();
    let mut seen_id = false;

    for _ in 0..map_length(decoder)? {
        let key = decoder.str().map_err(cbor_error)?;
        match (key, id) {
            ("id", EntityId::Text) => {
                entity.text_id = decoder.str().map_err(cbor_error)?.to_string();
                seen_id = true;
            }
            ("id", EntityId::Bytes) => {
                entity.binary_id = decoder.bytes().map_err(cbor_error)?.to_vec();
                seen_id = true;
            }
            ("name", _) => entity.name = Some(decoder.str().map_err(cbor_error)?.to_string()),
            ("displayName", _) => {
                entity.display_name = Some(decoder.str().map_err(cbor_error)?.to_string());
            }
            _ => decoder.skip().map_err(cbor_error)?,
        }
    }

    if !seen_id {
        return Err(CtapStatus::MissingParameter.into());
    }
    Ok(entity)
}

/// Read `pubKeyCredParams`, keeping the algorithms of `public-key` entries in order.
fn parse_credential_parameters(decoder: &mut Decoder<'_>) -> Result<Vec<i32>> {
    let mut algorithms = Vec::new();
    for _ in 0..array_length(decoder)? {
        let mut algorithm = None;
        let mut is_public_key = false;
        for _ in 0..map_length(decoder)? {
            match decoder.str().map_err(cbor_error)? {
                "alg" => algorithm = Some(decoder.i32().map_err(cbor_error)?),
                "type" => is_public_key = decoder.str().map_err(cbor_error)? == "public-key",
                _ => decoder.skip().map_err(cbor_error)?,
            }
        }
        if let Some(algorithm) = algorithm.filter(|_| is_public_key) {
            algorithms.push(algorithm);
        }
    }
    Ok(algorithms)
}

/// Read an `excludeList` or `allowList`, keeping the `public-key` credential IDs.
fn parse_credential_list(decoder: &mut Decoder<'_>) -> Result<Vec<Vec<u8>>> {
    let mut credentials = Vec::new();
    for _ in 0..array_length(decoder)? {
        let mut id = None;
        let mut is_public_key = false;
        for _ in 0..map_length(decoder)? {
            match decoder.str().map_err(cbor_error)? {
                "id" => id = Some(decoder.bytes().map_err(cbor_error)?.to_vec()),
                "type" => is_public_key = decoder.str().map_err(cbor_error)? == "public-key",
                _ => decoder.skip().map_err(cbor_error)?,
            }
        }
        if let Some(id) = id.filter(|_| is_public_key) {
            credentials.push(id);
        }
    }
    Ok(credentials)
}

/// The option keys a request may carry.
///
/// Only `up` changes what the authenticator does. `rk` and `uv` are read and
/// ignored: every credential here is discoverable, and every ceremony that
/// prompts is user-verified by the database's own password.
#[derive(Default)]
struct Options {
    user_presence: Option<bool>,
}

fn parse_options(decoder: &mut Decoder<'_>) -> Result<Options> {
    let mut options = Options::default();
    for _ in 0..map_length(decoder)? {
        let key = decoder.str().map_err(cbor_error)?;
        let value = decoder.bool().map_err(cbor_error)?;
        if key == "up" {
            options.user_presence = Some(value);
        }
    }
    Ok(options)
}

/// CTAP2 mandates definite lengths, so an indefinite map is malformed.
fn map_length(decoder: &mut Decoder<'_>) -> Result<u64> {
    decoder
        .map()
        .map_err(cbor_error)?
        .ok_or(CtapError::new(CtapStatus::InvalidCbor))
}

fn array_length(decoder: &mut Decoder<'_>) -> Result<u64> {
    decoder
        .array()
        .map_err(cbor_error)?
        .ok_or(CtapError::new(CtapStatus::InvalidCbor))
}

fn cbor_error(error: minicbor::decode::Error) -> CtapError {
    if error.is_type_mismatch() {
        CtapStatus::CborUnexpectedType.into()
    } else {
        CtapStatus::InvalidCbor.into()
    }
}
