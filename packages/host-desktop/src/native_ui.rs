use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use keeless_core::{
    CoreError, HostFuture, PasskeyConsentMode, PasskeyConsentProvider, PasskeyConsentRequest,
    PasswordInputMode, PasswordInputProvider,
};
use keeless_lesswire::{
    ApprovalProvider, MessageFrame, Server, ServerHost, StateStore, SystemClock, WireFuture,
};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    sync::{Mutex, watch},
};
use zeroize::Zeroizing;

#[cfg(windows)]
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const MAX_STDOUT_BYTES: usize = 64 * 1024;
const MAX_STDERR_BYTES: usize = 16 * 1024;
const UI_TIMEOUT: Duration = Duration::from_secs(15 * 60);

pub struct NativeUi {
    executable: PathBuf,
    dialog: Mutex<()>,
    shutdown: watch::Receiver<bool>,
}

impl NativeUi {
    pub fn new(executable: PathBuf, shutdown: watch::Receiver<bool>) -> Self {
        Self {
            executable,
            dialog: Mutex::new(()),
            shutdown,
        }
    }

    async fn request_password(
        &self,
        mode: PasswordInputMode,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
        let request = PasswordRequest {
            mode: match mode {
                PasswordInputMode::Create => "create",
                PasswordInputMode::Unlock => "unlock",
                PasswordInputMode::Reveal => "reveal",
                PasswordInputMode::Save => "save",
            },
        };
        let arguments = serde_json::to_string(&request).map_err(|error| error.to_string())?;
        let plaintext = self.invoke("password", &arguments).await?;
        match response_status(&plaintext, "password")? {
            ResponseStatus::Cancelled => Ok(None),
            ResponseStatus::Error(error) => Err(error),
            ResponseStatus::Selected => {
                let response: PasswordSelected<'_> = decode_response(&plaintext)?;
                let password = Zeroizing::new(response.result.password.into_bytes());
                validate_selected(response.version, response.kind, response.status, "password")?;
                if password.len() > 4096 {
                    return Err("native UI returned an oversized password".into());
                }
                Ok(Some(password))
            }
        }
    }

    async fn request_approval(&self, bundle: &str) -> Result<bool, String> {
        let arguments = serde_json::to_string(&ConnectionRequest { public_key: bundle })
            .map_err(|error| error.to_string())?;
        let plaintext = self.invoke("connection", &arguments).await?;
        match response_status(&plaintext, "connection")? {
            ResponseStatus::Cancelled => Ok(false),
            ResponseStatus::Error(error) => Err(error),
            ResponseStatus::Selected => {
                let response: ConnectionSelected<'_> = decode_response(&plaintext)?;
                validate_selected(
                    response.version,
                    response.kind,
                    response.status,
                    "connection",
                )?;
                Ok(response.result.allowed)
            }
        }
    }

    async fn request_passkey_consent_ui(
        &self,
        request: PasskeyConsentRequest,
    ) -> Result<Option<usize>, String> {
        let accounts = request
            .accounts
            .iter()
            .enumerate()
            .map(|(index, username)| PasskeyAccount {
                id: index.to_string(),
                username: sanitize_label(username),
            })
            .collect::<Vec<_>>();
        let request = PasskeyRequest {
            mode: match request.mode {
                PasskeyConsentMode::Register => "register",
                PasskeyConsentMode::Assert => "assert",
            },
            rp_id: sanitize_label(&request.rp_id),
            accounts,
        };
        let arguments = serde_json::to_string(&request).map_err(|error| error.to_string())?;
        let plaintext = self.invoke("passkey", &arguments).await?;
        selected_passkey_index(&plaintext, request.accounts.len())
    }

    async fn invoke(&self, kind: &str, arguments: &str) -> Result<Zeroizing<Vec<u8>>, String> {
        let mut shutdown = self.shutdown.clone();
        let _dialog = tokio::select! {
            guard = self.dialog.lock() => guard,
            changed = shutdown.changed() => {
                let _ = changed;
                return Err("desktop host is shutting down".into());
            }
        };
        if *shutdown.borrow() {
            return Err("desktop host is shutting down".into());
        }

        let mut recipient = one_shot_server().await?;
        let mut command = native_ui_command(
            &self.executable,
            &recipient.public_key_bundle(),
            kind,
            arguments,
        );
        let mut child = command
            .spawn()
            .map_err(|error| format!("failed to start native UI: {error}"))?;
        let _stdin = child
            .stdin
            .take()
            .ok_or_else(|| "native UI stdin was not piped".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "native UI stdout was not piped".to_owned())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "native UI stderr was not piped".to_owned())?;

        let output = async {
            let (stdout, stderr, status) = tokio::join!(
                read_bounded(stdout, MAX_STDOUT_BYTES),
                read_bounded(stderr, MAX_STDERR_BYTES),
                child.wait(),
            );
            Ok::<_, String>((
                stdout.map_err(|error| format!("failed to read native UI stdout: {error}"))?,
                stderr.map_err(|error| format!("failed to read native UI stderr: {error}"))?,
                status.map_err(|error| format!("failed to wait for native UI: {error}"))?,
            ))
        };
        let result = tokio::select! {
            result = tokio::time::timeout(UI_TIMEOUT, output) => match result {
                Ok(result) => result,
                Err(_) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return Err("native UI timed out".into());
                }
            },
            changed = shutdown.changed() => {
                let _ = changed;
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err("desktop host is shutting down".into());
            }
        }?;
        let (stdout, stderr, status) = result;
        if !status.success() {
            let detail = String::from_utf8_lossy(&stderr);
            let detail = detail.trim();
            return Err(if detail.is_empty() {
                format!("native UI exited with {status}")
            } else {
                format!("native UI exited with {status}: {detail}")
            });
        }
        let frame: MessageFrame = serde_json::from_slice(trim_ascii(&stdout))
            .map_err(|error| format!("native UI returned an invalid frame: {error}"))?;
        recipient
            .add_runtime_approval(&frame.public_key)
            .map_err(|error| error.to_string())?;
        let captured = Arc::new(StdMutex::new(None));
        let output = captured.clone();
        recipient
            .handle_frame(&frame, move |_owner, plaintext| {
                *output.lock().expect("native UI output lock poisoned") = Some(plaintext);
                async { Ok::<Option<Vec<u8>>, String>(None) }
            })
            .await
            .map_err(|error| error.to_string())?;
        captured
            .lock()
            .map_err(|_| "native UI output lock was poisoned".to_owned())?
            .take()
            .ok_or_else(|| "native UI returned an unauthenticated response".into())
    }
}

impl PasswordInputProvider for NativeUi {
    fn request_password(
        &self,
        mode: PasswordInputMode,
    ) -> HostFuture<'_, keeless_core::Result<Option<Zeroizing<Vec<u8>>>>> {
        Box::pin(async move { self.request_password(mode).await.map_err(CoreError::Host) })
    }
}

impl PasskeyConsentProvider for NativeUi {
    fn request_passkey_consent(
        &self,
        request: PasskeyConsentRequest,
    ) -> HostFuture<'_, keeless_core::Result<Option<usize>>> {
        Box::pin(async move {
            self.request_passkey_consent_ui(request)
                .await
                .map_err(CoreError::Host)
        })
    }
}

impl ApprovalProvider for NativeUi {
    fn approve(&self, public_key_bundle: &str) -> WireFuture<'_, keeless_lesswire::Result<bool>> {
        let bundle = public_key_bundle.to_owned();
        Box::pin(async move {
            self.request_approval(&bundle)
                .await
                .map_err(keeless_lesswire::Error::Host)
        })
    }
}

fn native_ui_command(path: &Path, public_key: &str, kind: &str, arguments: &str) -> Command {
    let mut command = Command::new(path);
    command
        .arg("--public-key")
        .arg(public_key)
        .arg(kind)
        .arg(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

async fn read_bounded(reader: impl AsyncRead + Unpin, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > limit {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "output exceeded the allowed size",
        ))
    } else {
        Ok(bytes)
    }
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    bytes.trim_ascii()
}

async fn one_shot_server() -> Result<Server, String> {
    Server::new(ServerHost {
        store: Arc::new(MemoryStore::default()),
        approval_provider: Arc::new(DenyApproval),
        clock: Arc::new(SystemClock),
        runtime_approved_clients: Vec::new(),
    })
    .await
    .map_err(|error| error.to_string())
}

#[derive(Default)]
struct MemoryStore(StdMutex<Option<Zeroizing<Vec<u8>>>>);

impl StateStore for MemoryStore {
    fn load(&self) -> WireFuture<'_, keeless_lesswire::Result<Option<Vec<u8>>>> {
        Box::pin(async {
            Ok(self
                .0
                .lock()
                .map_err(|_| keeless_lesswire::Error::Host("UI state lock was poisoned".into()))?
                .as_ref()
                .map(|value| value.to_vec()))
        })
    }

    fn save<'a>(&'a self, value: &'a [u8]) -> WireFuture<'a, keeless_lesswire::Result<()>> {
        Box::pin(async move {
            *self.0.lock().map_err(|_| {
                keeless_lesswire::Error::Host("UI state lock was poisoned".into())
            })? = Some(Zeroizing::new(value.to_vec()));
            Ok(())
        })
    }
}

struct DenyApproval;

impl ApprovalProvider for DenyApproval {
    fn approve(&self, _: &str) -> WireFuture<'_, keeless_lesswire::Result<bool>> {
        Box::pin(async { Ok(false) })
    }
}

#[derive(Serialize)]
struct PasswordRequest {
    mode: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PasskeyRequest {
    mode: &'static str,
    rp_id: String,
    accounts: Vec<PasskeyAccount>,
}

#[derive(Serialize)]
struct PasskeyAccount {
    id: String,
    username: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionRequest<'a> {
    public_key: &'a str,
}

enum ResponseStatus {
    Selected,
    Cancelled,
    Error(String),
}

fn response_status(bytes: &[u8], expected_kind: &str) -> Result<ResponseStatus, String> {
    let response: ResponseHeader<'_> = decode_response(bytes)?;
    if response.version != 1 || response.kind != expected_kind {
        return Err("native UI returned a mismatched response".into());
    }
    match response.status {
        "selected" => Ok(ResponseStatus::Selected),
        "cancelled" => Ok(ResponseStatus::Cancelled),
        "error" => {
            let error = response
                .error
                .ok_or_else(|| "native UI error response is missing details".to_owned())?;
            Ok(ResponseStatus::Error(format!(
                "native UI {}: {}",
                error.code, error.message
            )))
        }
        _ => Err("native UI returned an unknown status".into()),
    }
}

fn validate_selected(
    version: u8,
    kind: &str,
    status: &str,
    expected_kind: &str,
) -> Result<(), String> {
    if version == 1 && kind == expected_kind && status == "selected" {
        Ok(())
    } else {
        Err("native UI returned a mismatched selected response".into())
    }
}

fn selected_passkey_index(bytes: &[u8], account_count: usize) -> Result<Option<usize>, String> {
    match response_status(bytes, "passkey")? {
        ResponseStatus::Cancelled => Ok(None),
        ResponseStatus::Error(error) => Err(error),
        ResponseStatus::Selected => {
            let response: PasskeySelected<'_> = decode_response(bytes)?;
            validate_selected(response.version, response.kind, response.status, "passkey")?;
            let index = response
                .result
                .account_id
                .parse::<usize>()
                .map_err(|_| "native UI returned an invalid passkey selection".to_owned())?;
            if response.result.account_id != index.to_string() || index >= account_count {
                return Err("native UI returned an invalid passkey selection".into());
            }
            Ok(Some(index))
        }
    }
}

fn decode_response<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, String> {
    serde_json::from_slice(bytes).map_err(|error| format!("invalid native UI response: {error}"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseHeader<'a> {
    version: u8,
    #[serde(borrow)]
    kind: &'a str,
    #[serde(borrow)]
    status: &'a str,
    #[serde(default, borrow)]
    error: Option<ResponseError<'a>>,
    #[serde(default)]
    #[serde(rename = "result")]
    _result: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseError<'a> {
    #[serde(borrow)]
    code: &'a str,
    message: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordSelected<'a> {
    version: u8,
    #[serde(borrow)]
    kind: &'a str,
    #[serde(borrow)]
    status: &'a str,
    result: PasswordResult,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordResult {
    password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasskeySelected<'a> {
    version: u8,
    #[serde(borrow)]
    kind: &'a str,
    #[serde(borrow)]
    status: &'a str,
    result: PasskeyResult<'a>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PasskeyResult<'a> {
    #[serde(borrow)]
    account_id: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectionSelected<'a> {
    version: u8,
    #[serde(borrow)]
    kind: &'a str,
    #[serde(borrow)]
    status: &'a str,
    result: ConnectionResult,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectionResult {
    allowed: bool,
}

/// Replace values native-ui would reject without changing their account ordering.
fn sanitize_label(value: &str) -> String {
    const MAX_LABEL_BYTES: usize = 256;

    let mut cleaned: String = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(character,
                    '\u{061c}' | '\u{200e}' | '\u{200f}'
                    | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                ' '
            } else {
                character
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return "(unnamed)".into();
    }
    if trimmed.len() != cleaned.len() {
        cleaned = trimmed.to_string();
    }
    while cleaned.len() > MAX_LABEL_BYTES {
        cleaned.pop();
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn command_uses_private_standard_streams() {
        let command = native_ui_command(
            Path::new("native-ui"),
            "v1.key.key",
            "password",
            r#"{"mode":"unlock"}"#,
        );
        assert_eq!(command.as_std().get_program(), OsStr::new("native-ui"));
        assert_eq!(
            command.as_std().get_args().collect::<Vec<_>>(),
            [
                OsStr::new("--public-key"),
                OsStr::new("v1.key.key"),
                OsStr::new("password"),
                OsStr::new(r#"{"mode":"unlock"}"#),
            ]
        );
    }

    #[test]
    fn validates_response_kind_and_status() {
        assert!(matches!(
            response_status(
                br#"{"version":1,"kind":"password","status":"cancelled"}"#,
                "password"
            ),
            Ok(ResponseStatus::Cancelled)
        ));
        assert!(
            response_status(
                br#"{"version":1,"kind":"connection","status":"cancelled"}"#,
                "password"
            )
            .is_err()
        );
    }

    #[test]
    fn decodes_escaped_response_strings() {
        let password: PasswordSelected<'_> = decode_response(
            br#"{"version":1,"kind":"password","status":"selected","result":{"password":"a\\b\"c"}}"#,
        )
        .unwrap();
        assert_eq!(password.result.password, r#"a\b"c"#);

        assert!(matches!(
            response_status(
                br#"{"version":1,"kind":"connection","status":"error","error":{"code":"ui_unavailable","message":"display unavailable"}}"#,
                "connection"
            ),
            Ok(ResponseStatus::Error(error)) if error == "native UI ui_unavailable: display unavailable"
        ));
    }

    #[test]
    fn decodes_only_indexed_passkey_selections() {
        let selected: PasskeySelected<'_> = decode_response(
            br#"{"version":1,"kind":"passkey","status":"selected","result":{"accountId":"1"}}"#,
        )
        .unwrap();
        validate_selected(selected.version, selected.kind, selected.status, "passkey").unwrap();
        assert_eq!(selected.result.account_id, "1");
        assert_eq!(sanitize_label("safe\u{202e}evil"), "safe evil");
    }

    #[test]
    fn accepts_only_a_canonical_in_range_passkey_selection() {
        assert_eq!(
            selected_passkey_index(
                br#"{"version":1,"kind":"passkey","status":"selected","result":{"accountId":"1"}}"#,
                2,
            )
            .unwrap(),
            Some(1)
        );
        assert!(
            selected_passkey_index(
                br#"{"version":1,"kind":"passkey","status":"selected","result":{"accountId":"2"}}"#,
                2,
            )
            .is_err()
        );
        assert!(selected_passkey_index(
            br#"{"version":1,"kind":"passkey","status":"selected","result":{"accountId":"01"}}"#,
            2,
        )
        .is_err());
        assert_eq!(
            selected_passkey_index(br#"{"version":1,"kind":"passkey","status":"cancelled"}"#, 2,)
                .unwrap(),
            None
        );
    }

    #[test]
    fn sanitizes_labels_without_changing_account_order() {
        assert_eq!(sanitize_label("alice\nbob"), "alice bob");
        assert_eq!(sanitize_label("\u{202e}"), "(unnamed)");
    }
}
