use std::{ffi::OsString, path::PathBuf};

use keeless_lesswire::PublicKeyBundle;
use serde::{Deserialize, Serialize, Serializer, ser::Error as _};
use zeroize::Zeroizing;

use crate::{Error, secure_text_edit::SecureTextBuffer, serialize_json};

const MAX_ARGUMENTS_BYTES: usize = 64 * 1024;
const MAX_LABEL_BYTES: usize = 256;
const MAX_FILTERS: usize = 32;
const MAX_EXTENSIONS: usize = 32;

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
            "file" => parse_request(&json).map(UiRequest::File),
            "connection" => parse_request(&json).map(UiRequest::Connection),
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
    File(FileRequest),
    Connection(ConnectionRequest),
}

impl UiRequest {
    fn validate(&self) -> Result<(), Error> {
        match self {
            Self::Password(_) => Ok(()),
            Self::File(request) => request.validate(),
            Self::Connection(request) => request.validate(),
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum FileMode {
    Open,
    Save,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileRequest {
    pub mode: FileMode,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub filters: Vec<FileFilter>,
    #[serde(default)]
    pub directory: Option<PathBuf>,
    #[serde(default)]
    pub file_name: Option<String>,
}

impl FileRequest {
    fn validate(&self) -> Result<(), Error> {
        validate_label(self.title.as_deref(), "title")?;
        validate_label(self.file_name.as_deref(), "fileName")?;
        if self
            .directory
            .as_ref()
            .is_some_and(|path| path.to_string_lossy().contains('\0'))
        {
            return Err(Error::InvalidRequest(
                "directory must not contain NUL characters".into(),
            ));
        }
        if self
            .file_name
            .as_ref()
            .is_some_and(|name| name.contains(['/', '\\']))
        {
            return Err(Error::InvalidRequest(
                "fileName must not contain path separators".into(),
            ));
        }
        if self.filters.len() > MAX_FILTERS {
            return Err(Error::InvalidRequest(format!(
                "filters must contain at most {MAX_FILTERS} entries"
            )));
        }
        for filter in &self.filters {
            validate_label(Some(&filter.name), "filter name")?;
            if filter.extensions.is_empty() || filter.extensions.len() > MAX_EXTENSIONS {
                return Err(Error::InvalidRequest(format!(
                    "each filter must contain 1 to {MAX_EXTENSIONS} extensions"
                )));
            }
            for extension in &filter.extensions {
                if extension.is_empty()
                    || extension.len() > 32
                    || !extension
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'*')
                {
                    return Err(Error::InvalidRequest(format!(
                        "invalid file extension: {extension}"
                    )));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectionRequest {
    pub public_key: String,
    #[serde(default)]
    pub name: Option<String>,
}

impl ConnectionRequest {
    fn validate(&self) -> Result<(), Error> {
        if PublicKeyBundle::parse(&self.public_key).is_none() {
            return Err(Error::InvalidRequest(
                "connection publicKey must be a canonical lesswire bundle".into(),
            ));
        }
        validate_label(self.name.as_deref(), "name")
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
    File(Result<Option<String>, String>),
    Connection(Option<bool>),
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

    pub fn file(value: Option<PathBuf>) -> Self {
        Self::File(match value {
            Some(path) => match absolute_path(path) {
                Ok(path) => path
                    .into_os_string()
                    .into_string()
                    .map(Some)
                    .map_err(|_| "selected path is not valid UTF-8".into()),
                Err(error) => Err(error.to_string()),
            },
            None => Ok(None),
        })
    }

    pub fn connection(value: Option<bool>) -> Self {
        Self::Connection(value)
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
            Self::File(Ok(Some(path))) => serialize_json(&SelectedResponse {
                version: 1,
                kind: "file",
                status: "selected",
                result: FileResult { path },
            }),
            Self::File(Ok(None)) => serialize_json(&StatusResponse {
                version: 1,
                kind: "file",
                status: "cancelled",
            }),
            Self::File(Err(message)) => serialize_json(&ErrorResponse {
                version: 1,
                kind: "file",
                status: "error",
                error: ResponseError {
                    code: "invalid_path",
                    message,
                },
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

fn absolute_path(path: PathBuf) -> std::io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        std::env::current_dir().map(|directory| directory.join(path))
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
struct FileResult<T> {
    path: T,
}

#[derive(Serialize)]
struct ConnectionResult {
    allowed: bool,
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
