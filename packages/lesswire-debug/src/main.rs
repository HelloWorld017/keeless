use std::{
    ffi::OsString,
    io::{self, Read, Write},
    process::ExitCode,
    sync::Arc,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use keeless_lesswire::{
    Client, Clock, Identity, KeyScope, MAX_FRAME_SIZE, MessageFrame, PublicKeyBundle, SystemClock,
};
use serde::Serialize;
use thiserror::Error;
use zeroize::Zeroizing;

const USAGE: &str = "\
Usage:
  lesswire-debug generate
  lesswire-debug public-key --identity <base64url-identity>
  lesswire-debug encrypt --public-key <bundle> [--identity <base64url-identity>]
  lesswire-debug decrypt --identity <base64url-identity>

encrypt reads plaintext bytes from stdin and writes a MessageFrame JSON line.
decrypt reads a MessageFrame JSON document from stdin and writes plaintext bytes.";

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1), io::stdin(), io::stdout()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lesswire-debug: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}

#[derive(Debug, Error)]
enum CliError {
    #[error("{0}\n\n{USAGE}")]
    Usage(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("lesswire failed: {0}")]
    Wire(#[from] keeless_lesswire::Error),
}

impl CliError {
    fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::InvalidInput(_) | Self::Io(_) | Self::Json(_) | Self::Wire(_) => 1,
        }
    }
}

enum Command {
    Help,
    Generate,
    PublicKey {
        identity: String,
    },
    Encrypt {
        public_key: String,
        identity: Option<String>,
    },
    Decrypt {
        identity: String,
    },
}

fn run(
    args: impl IntoIterator<Item = OsString>,
    mut input: impl Read,
    mut output: impl Write,
) -> Result<(), CliError> {
    match parse_args(args)? {
        Command::Help => {
            writeln!(output, "{USAGE}")?;
        }
        Command::Generate => {
            let identity = Identity::generate(KeyScope::App)?;
            write_identity(&mut output, &identity)?;
        }
        Command::PublicKey { identity } => {
            let identity = decode_identity(&identity)?;
            write_json(
                &mut output,
                &PublicKeyOutput {
                    public_key: &identity.public_key_bundle(),
                },
            )?;
        }
        Command::Encrypt {
            public_key,
            identity,
        } => {
            let recipient = PublicKeyBundle::parse(&public_key).ok_or_else(|| {
                CliError::InvalidInput("invalid recipient public-key bundle".into())
            })?;
            let identity = identity
                .as_deref()
                .map(decode_identity)
                .transpose()?
                .map_or_else(|| Identity::generate(KeyScope::App), Ok)?;
            let plaintext = read_bounded(&mut input)?;
            let frame = encrypt(identity, &recipient, &plaintext, Arc::new(SystemClock))?;
            let encoded = serde_json::to_vec(&frame)?;
            if encoded.len() > MAX_FRAME_SIZE {
                return Err(CliError::InvalidInput(format!(
                    "encrypted frame exceeds {MAX_FRAME_SIZE} bytes"
                )));
            }
            output.write_all(&encoded)?;
            output.write_all(b"\n")?;
        }
        Command::Decrypt { identity } => {
            let identity = decode_identity(&identity)?;
            let encoded = read_bounded(&mut input)?;
            let frame: MessageFrame = serde_json::from_slice(&encoded)?;
            let clock: Arc<dyn Clock> = Arc::new(FrameClock(frame.timestamp));
            let plaintext = decrypt(identity, &frame, clock)?.ok_or_else(|| {
                CliError::InvalidInput(
                    "frame was rejected (invalid or encrypted for another identity)".into(),
                )
            })?;
            output.write_all(&plaintext)?;
        }
    }
    output.flush()?;
    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Command, CliError> {
    let args = args
        .into_iter()
        .map(|value| {
            value
                .into_string()
                .map_err(|_| CliError::Usage("arguments must be valid UTF-8".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let Some((command, rest)) = args.split_first() else {
        return Err(CliError::Usage("missing command".into()));
    };
    match command.as_str() {
        "--help" | "-h" | "help" if rest.is_empty() => Ok(Command::Help),
        "generate" if rest.is_empty() => Ok(Command::Generate),
        "public-key" => {
            let mut identity = None;
            parse_options(rest, |flag, value| match flag {
                "--identity" => set_value(&mut identity, flag, value),
                _ => Err(CliError::Usage(format!("unknown option: {flag}"))),
            })?;
            Ok(Command::PublicKey {
                identity: required(identity, "--identity")?,
            })
        }
        "encrypt" => {
            let mut public_key = None;
            let mut identity = None;
            parse_options(rest, |flag, value| match flag {
                "--public-key" => set_value(&mut public_key, flag, value),
                "--identity" => set_value(&mut identity, flag, value),
                _ => Err(CliError::Usage(format!("unknown option: {flag}"))),
            })?;
            Ok(Command::Encrypt {
                public_key: required(public_key, "--public-key")?,
                identity,
            })
        }
        "decrypt" => {
            let mut identity = None;
            parse_options(rest, |flag, value| match flag {
                "--identity" => set_value(&mut identity, flag, value),
                _ => Err(CliError::Usage(format!("unknown option: {flag}"))),
            })?;
            Ok(Command::Decrypt {
                identity: required(identity, "--identity")?,
            })
        }
        _ if matches!(command.as_str(), "generate" | "--help" | "-h" | "help") => Err(
            CliError::Usage(format!("command {command} does not accept arguments")),
        ),
        _ => Err(CliError::Usage(format!("unknown command: {command}"))),
    }
}

fn parse_options(
    args: &[String],
    mut use_option: impl FnMut(&str, String) -> Result<(), CliError>,
) -> Result<(), CliError> {
    let mut index = 0;
    while index < args.len() {
        let flag = &args[index];
        let value = args
            .get(index + 1)
            .ok_or_else(|| CliError::Usage(format!("{flag} requires a value")))?;
        use_option(flag, value.clone())?;
        index += 2;
    }
    Ok(())
}

fn set_value(target: &mut Option<String>, flag: &str, value: String) -> Result<(), CliError> {
    if target.replace(value).is_some() {
        return Err(CliError::Usage(format!("duplicate option: {flag}")));
    }
    Ok(())
}

fn required(value: Option<String>, flag: &str) -> Result<String, CliError> {
    value.ok_or_else(|| CliError::Usage(format!("missing required option: {flag}")))
}

fn encode_identity(identity: &Identity) -> Zeroizing<String> {
    let bytes = identity.to_bytes();
    Zeroizing::new(URL_SAFE_NO_PAD.encode(&bytes[..]))
}

fn decode_identity(value: &str) -> Result<Identity, CliError> {
    let bytes = Zeroizing::new(URL_SAFE_NO_PAD.decode(value).map_err(|_| {
        CliError::InvalidInput("identity must be base64url without padding".into())
    })?);
    if URL_SAFE_NO_PAD.encode(&bytes[..]) != value {
        return Err(CliError::InvalidInput(
            "identity must use canonical base64url without padding".into(),
        ));
    }
    Identity::from_bytes(KeyScope::App, &bytes).map_err(CliError::Wire)
}

fn read_bounded(input: &mut impl Read) -> Result<Zeroizing<Vec<u8>>, CliError> {
    let mut bytes = Zeroizing::new(Vec::new());
    input
        .take(MAX_FRAME_SIZE as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FRAME_SIZE {
        return Err(CliError::InvalidInput(format!(
            "stdin exceeds {MAX_FRAME_SIZE} bytes"
        )));
    }
    Ok(bytes)
}

fn encrypt(
    identity: Identity,
    recipient: &PublicKeyBundle,
    plaintext: &[u8],
    clock: Arc<dyn Clock>,
) -> Result<MessageFrame, CliError> {
    Client::new(identity, recipient.as_str(), clock)?
        .encrypt(plaintext)
        .map_err(CliError::Wire)
}

fn decrypt(
    identity: Identity,
    frame: &MessageFrame,
    clock: Arc<dyn Clock>,
) -> Result<Option<Zeroizing<Vec<u8>>>, CliError> {
    Client::new(identity, &frame.public_key, clock)?
        .decrypt(frame)
        .map_err(CliError::Wire)
}

fn write_identity(output: &mut impl Write, identity: &Identity) -> Result<(), CliError> {
    let encoded = encode_identity(identity);
    let public_key = identity.public_key_bundle();
    write_json(
        output,
        &IdentityOutput {
            identity: &encoded,
            public_key: &public_key,
        },
    )
}

fn write_json(output: &mut impl Write, value: &impl Serialize) -> Result<(), CliError> {
    serde_json::to_writer(&mut *output, value)?;
    output.write_all(b"\n")?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IdentityOutput<'a> {
    identity: &'a str,
    public_key: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicKeyOutput<'a> {
    public_key: &'a str,
}

struct FrameClock(i64);

impl Clock for FrameClock {
    fn now_millis(&self) -> i64 {
        self.0
    }

    fn monotonic_millis(&self) -> u64 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_encoding_round_trips() {
        let identity = Identity::from_secrets(KeyScope::App, [11; 32], [13; 32]);
        let encoded = encode_identity(&identity);
        let restored = decode_identity(&encoded).unwrap();
        assert_eq!(restored.public_key_bundle(), identity.public_key_bundle());
        assert!(decode_identity("not-an-identity").is_err());
    }

    #[test]
    fn decrypt_command_accepts_stale_frames() {
        let sender = Identity::from_secrets(KeyScope::App, [17; 32], [19; 32]);
        let recipient = Identity::from_secrets(KeyScope::App, [23; 32], [29; 32]);
        let recipient_bundle = PublicKeyBundle::parse(&recipient.public_key_bundle()).unwrap();
        let frame = encrypt(
            sender,
            &recipient_bundle,
            b"binary\0payload",
            Arc::new(FrameClock(10_000)),
        )
        .unwrap();

        let identity = encode_identity(&recipient);
        let mut output = Vec::new();
        run(
            ["decrypt", "--identity", identity.as_str()].map(OsString::from),
            serde_json::to_vec(&frame).unwrap().as_slice(),
            &mut output,
        )
        .unwrap();
        assert_eq!(output, b"binary\0payload");
    }

    #[test]
    fn command_parser_rejects_missing_values() {
        assert!(parse_args(["encrypt", "--public-key"].map(OsString::from)).is_err());
    }

    #[test]
    fn generate_and_public_key_commands_emit_json() {
        let mut generated = Vec::new();
        run(
            ["generate"].map(OsString::from),
            io::empty(),
            &mut generated,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&generated).unwrap();
        let identity = value["identity"].as_str().unwrap();

        let mut public = Vec::new();
        run(
            ["public-key", "--identity", identity].map(OsString::from),
            io::empty(),
            &mut public,
        )
        .unwrap();
        let public: serde_json::Value = serde_json::from_slice(&public).unwrap();
        assert_eq!(public["publicKey"], value["publicKey"]);
    }
}
