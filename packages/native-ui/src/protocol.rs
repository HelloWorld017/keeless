use std::ffi::OsString;

use keeless_lesswire::PublicKeyBundle;
use serde::{Deserialize, Serialize, Serializer, ser::Error as _};
use zeroize::Zeroizing;

use crate::{Error, secure_text_edit::SecureTextBuffer, serialize_json};

const MAX_ARGUMENTS_BYTES: usize = 64 * 1024;
const MAX_LABEL_BYTES: usize = 256;

pub struct Arguments {
    pub public_key: PublicKeyBundle,
    pub request: UiRequest,
}

impl Arguments {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, Error> {
        let mut args = args.into_iter();
        let flag = next_utf8(&mut args, "missing --public-key")?;
        if flag != "--public-key" {
            return Err(Error::Usage(
                "expected --public-key as the first argument".into(),
            ));
        }
        let public_key = next_utf8(&mut args, "--public-key requires a value")?;
        let public_key = PublicKeyBundle::parse(&public_key).ok_or_else(|| {
            Error::Usage("--public-key must be a canonical lesswire public key bundle".into())
        })?;
        let kind = next_utf8(&mut args, "missing UI kind")?;
        let json = next_utf8(&mut args, "missing UI arguments JSON")?;
        if json.len() > MAX_ARGUMENTS_BYTES {
            return Err(Error::InvalidRequest(format!(
                "UI arguments exceed {MAX_ARGUMENTS_BYTES} bytes"
            )));
        }
        if args.next().is_some() {
            return Err(Error::Usage("unexpected trailing arguments".into()));
        }

        let request = match kind.as_str() {
            "password" => parse_request(&json).map(UiRequest::Password),
            "connection" => parse_request(&json).map(UiRequest::Connection),
            "passkey" => parse_request(&json).map(UiRequest::Passkey),
            _ => return Err(Error::Usage(format!("unknown UI kind: {kind}"))),
        }?;
        request.validate()?;
        Ok(Self {
            public_key,
            request,
        })
    }
}

fn next_utf8(
    args: &mut impl Iterator<Item = OsString>,
    missing: &'static str,
) -> Result<String, Error> {
    args.next()
        .ok_or_else(|| Error::Usage(missing.into()))?
        .into_string()
        .map_err(|_| Error::Usage("arguments must be valid UTF-8".into()))
}

fn parse_request<T: for<'de> Deserialize<'de>>(json: &str) -> Result<T, Error> {
    serde_json::from_str(json).map_err(|error| Error::InvalidRequest(error.to_string()))
}

pub enum UiRequest {
    Password(PasswordRequest),
    Connection(ConnectionRequest),
    Passkey(PasskeyRequest),
}

impl UiRequest {
    fn validate(&self) -> Result<(), Error> {
        match self {
            Self::Password(_) => Ok(()),
            Self::Connection(request) => request.validate(),
            Self::Passkey(request) => request.validate(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PasswordMode {
    Create,
    Unlock,
    Reveal,
    Save,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordRequest {
    pub mode: PasswordMode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectionRequest {
    pub public_key: String,
    pub sender_scope: String,
    pub recipient: String,
    pub recipient_scope: String,
    pub kind: String,
}

impl ConnectionRequest {
    fn validate(&self) -> Result<(), Error> {
        let Some(sender) = PublicKeyBundle::parse(&self.public_key) else {
            return Err(Error::InvalidRequest(
                "connection publicKey must be a canonical lesswire bundle".into(),
            ));
        };
        let Some(recipient) = PublicKeyBundle::parse(&self.recipient) else {
            return Err(Error::InvalidRequest(
                "connection recipient must be a canonical lesswire bundle".into(),
            ));
        };
        if self.sender_scope != scope_name(sender.scope)
            || self.recipient_scope != scope_name(recipient.scope)
            || !matches!(self.kind.as_str(), "initial" | "upgrade")
        {
            return Err(Error::InvalidRequest(
                "connection scope or kind is invalid".into(),
            ));
        }
        Ok(())
    }
}

fn scope_name(scope: keeless_lesswire::KeyScope) -> &'static str {
    match scope {
        keeless_lesswire::KeyScope::CoreUntrusted => "core_untrusted",
        keeless_lesswire::KeyScope::Core => "core",
        keeless_lesswire::KeyScope::App => "app",
        keeless_lesswire::KeyScope::Passkey => "passkey",
    }
}

/// Largest number of accounts a passkey prompt will list.
///
/// Matches what the CTAP layer will send at most, and keeps an unbounded list
/// from producing a dialog the user cannot dismiss.
const MAX_PASSKEY_ACCOUNTS: usize = 32;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PasskeyMode {
    /// Confirm creating a new passkey for a relying party.
    Register,
    /// Confirm signing in, choosing among the accounts for a relying party.
    Assert,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PasskeyRequest {
    pub mode: PasskeyMode,
    pub rp_id: String,
    pub accounts: Vec<PasskeyAccount>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PasskeyAccount {
    /// Opaque handle the caller uses to identify the chosen account.
    pub id: String,
    pub username: String,
}

impl PasskeyRequest {
    fn validate(&self) -> Result<(), Error> {
        validate_label(Some(&self.rp_id), "rpId")?;
        let expected = match self.mode {
            PasskeyMode::Register => self.accounts.len() == 1,
            PasskeyMode::Assert => (1..=MAX_PASSKEY_ACCOUNTS).contains(&self.accounts.len()),
        };
        if !expected {
            return Err(Error::InvalidRequest(format!(
                "passkey accounts must contain {} entries",
                match self.mode {
                    PasskeyMode::Register => "exactly 1".to_string(),
                    PasskeyMode::Assert => format!("1 to {MAX_PASSKEY_ACCOUNTS}"),
                }
            )));
        }
        for account in &self.accounts {
            validate_label(Some(&account.id), "account id")?;
            validate_label(Some(&account.username), "account username")?;
        }
        Ok(())
    }
}

fn validate_label(value: Option<&str>, field: &str) -> Result<(), Error> {
    if value.is_some_and(|value| {
        value.is_empty()
            || value.len() > MAX_LABEL_BYTES
            || value.chars().any(|character| {
                character.is_control()
                    || matches!(
                        character,
                        '\u{061c}'
                            | '\u{200e}'
                            | '\u{200f}'
                            | '\u{202a}'..='\u{202e}'
                            | '\u{2066}'..='\u{2069}'
                    )
            })
    }) {
        return Err(Error::InvalidRequest(format!(
            "{field} must contain 1 to {MAX_LABEL_BYTES} bytes without control or bidi characters"
        )));
    }
    Ok(())
}

pub enum PlaintextResponse {
    Password(Option<SecureTextBuffer>),
    Connection(Option<bool>),
    /// The chosen account's ID, or `None` when the user refused.
    Passkey(Option<String>),
    Error {
        kind: &'static str,
        code: &'static str,
        message: String,
    },
}

impl PlaintextResponse {
    pub fn password(value: Option<SecureTextBuffer>) -> Self {
        Self::Password(value)
    }

    pub fn connection(value: Option<bool>) -> Self {
        Self::Connection(value)
    }

    pub fn passkey(value: Option<String>) -> Self {
        Self::Passkey(value)
    }

    pub fn error(kind: &'static str, code: &'static str, message: impl ToString) -> Self {
        Self::Error {
            kind,
            code,
            message: message.to_string(),
        }
    }

    pub fn to_json(&self) -> Result<Zeroizing<Vec<u8>>, serde_json::Error> {
        match self {
            Self::Password(Some(password)) => serialize_json(&SelectedResponse {
                version: 1,
                kind: "password",
                status: "selected",
                result: PasswordResult {
                    password: SecretRef(password),
                },
            }),
            Self::Password(None) => serialize_json(&StatusResponse {
                version: 1,
                kind: "password",
                status: "cancelled",
            }),
            Self::Connection(Some(allowed)) => serialize_json(&SelectedResponse {
                version: 1,
                kind: "connection",
                status: "selected",
                result: ConnectionResult { allowed: *allowed },
            }),
            Self::Connection(None) => serialize_json(&StatusResponse {
                version: 1,
                kind: "connection",
                status: "cancelled",
            }),
            Self::Passkey(Some(account_id)) => serialize_json(&SelectedResponse {
                version: 1,
                kind: "passkey",
                status: "selected",
                result: PasskeyResult { account_id },
            }),
            Self::Passkey(None) => serialize_json(&StatusResponse {
                version: 1,
                kind: "passkey",
                status: "cancelled",
            }),
            Self::Error {
                kind,
                code,
                message,
            } => serialize_json(&ErrorResponse {
                version: 1,
                kind,
                status: "error",
                error: ResponseError { code, message },
            }),
        }
    }
}

struct SecretRef<'a>(&'a SecureTextBuffer);

impl Serialize for SecretRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0
            .with_str(|value| serializer.serialize_str(value))
            .map_err(S::Error::custom)?
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectedResponse<T> {
    version: u8,
    kind: &'static str,
    status: &'static str,
    result: T,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    version: u8,
    kind: &'static str,
    status: &'static str,
}

#[derive(Serialize)]
struct PasswordResult<T> {
    password: T,
}

#[derive(Serialize)]
struct ConnectionResult {
    allowed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PasskeyResult<'a> {
    account_id: &'a str,
}

#[derive(Serialize)]
struct ErrorResponse<'a> {
    version: u8,
    kind: &'static str,
    status: &'static str,
    error: ResponseError<'a>,
}

#[derive(Serialize)]
struct ResponseError<'a> {
    code: &'static str,
    message: &'a str,
}
