mod dialogs;
mod protocol;
mod secure_text_edit;

use std::{
    ffi::OsString,
    io::{self, Write},
    sync::Arc,
};

use keeless_lesswire::{Client, Identity, PublicKeyBundle, SystemClock};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::{
    dialogs::{prompt_connection, prompt_password},
    protocol::{Arguments, PlaintextResponse, UiRequest},
};

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Usage(String),
    #[error("invalid UI request: {0}")]
    InvalidRequest(String),
    #[error("failed to initialize encryption: {0}")]
    Encryption(keeless_lesswire::Error),
    #[error("failed to serialize response: {0}")]
    Serialization(serde_json::Error),
    #[error("failed to write response: {0}")]
    Output(io::Error),
}

impl Error {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) | Self::InvalidRequest(_) => 2,
            Self::Encryption(_) | Self::Serialization(_) | Self::Output(_) => 1,
        }
    }
}

pub fn run(args: impl IntoIterator<Item = OsString>) -> Result<(), Error> {
    let arguments = Arguments::parse(args)?;
    let response = match arguments.request {
        UiRequest::Password(request) => match prompt_password(request) {
            Ok(password) => PlaintextResponse::password(password),
            Err(error) => PlaintextResponse::error("password", "ui_unavailable", error),
        },
        UiRequest::Connection(request) => match prompt_connection(request) {
            Ok(allowed) => PlaintextResponse::connection(allowed),
            Err(error) => PlaintextResponse::error("connection", "ui_unavailable", error),
        },
    };

    let plaintext = response.to_json().map_err(Error::Serialization)?;
    let frame = encrypt(&arguments.public_key, &plaintext)?;
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &frame).map_err(Error::Serialization)?;
    stdout.write_all(b"\n").map_err(Error::Output)?;
    stdout.flush().map_err(Error::Output)
}

fn encrypt(
    recipient: &PublicKeyBundle,
    plaintext: &[u8],
) -> Result<keeless_lesswire::MessageFrame, Error> {
    let identity = Identity::generate().map_err(Error::Encryption)?;
    let client = Client::new(identity, Some(recipient.as_str()), Arc::new(SystemClock))
        .map_err(Error::Encryption)?;
    client.encrypt(plaintext).map_err(Error::Encryption)
}

fn serialize_json<T: serde::Serialize>(value: &T) -> Result<Zeroizing<Vec<u8>>, serde_json::Error> {
    let mut output = Zeroizing::new(Vec::new());
    serde_json::to_writer(&mut *output, value)?;
    Ok(output)
}

#[cfg(test)]
mod tests;
