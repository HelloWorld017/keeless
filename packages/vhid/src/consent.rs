//! Asking the user to approve a passkey ceremony.
//!
//! The prompt is a `keeless-native-ui` child process that answers with one
//! encrypted frame. The daemon owns it — rather than delegating to the app —
//! because only the owner can dismiss the dialog when the browser cancels.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use keeless_lesswire::{
    ApprovalProvider, MessageFrame, Server, ServerHost, StateStore, SystemClock, WireFuture,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::Mutex;

const MAX_STDOUT_BYTES: usize = 64 * 1024;
const MAX_STDERR_BYTES: usize = 16 * 1024;

/// How long a prompt may stay open.
///
/// Browsers give a WebAuthn ceremony a minute or two before abandoning it, so a
/// prompt outliving that would linger with nothing left to answer.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(120);

/// One account the user can pick, identified by the entry that holds it.
#[derive(Clone, Debug, Serialize)]
pub struct Account {
    pub id: String,
    pub username: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ceremony {
    Register,
    Assert,
}

/// Spawns consent prompts, one at a time.
pub struct ConsentPrompt {
    executable: PathBuf,
    prompt: Mutex<()>,
}

impl ConsentPrompt {
    pub fn new(executable: PathBuf) -> Self {
        Self {
            executable,
            prompt: Mutex::new(()),
        }
    }

    /// Ask the user to approve a ceremony, returning the account they chose.
    ///
    /// `None` means they refused or closed the prompt. Dropping the returned
    /// future kills the child, which is how a cancelled ceremony takes its
    /// dialog down with it.
    pub async fn request(
        &self,
        ceremony: Ceremony,
        rp_id: &str,
        accounts: &[Account],
    ) -> Result<Option<String>, String> {
        let _guard = self.prompt.lock().await;
        let arguments = serde_json::to_string(&PasskeyRequest {
            mode: match ceremony {
                Ceremony::Register => "register",
                Ceremony::Assert => "assert",
            },
            rp_id,
            accounts,
        })
        .map_err(|error| error.to_string())?;

        let plaintext = self.invoke(&arguments).await?;
        let response: PasskeyResponse = serde_json::from_slice(&plaintext)
            .map_err(|error| format!("consent prompt returned invalid JSON: {error}"))?;
        if response.version != 1 || response.kind != "passkey" {
            return Err("consent prompt returned an unexpected response".into());
        }
        match response.status {
            "selected" => Ok(Some(
                response
                    .result
                    .ok_or("consent prompt selected nothing")?
                    .account_id,
            )),
            "cancelled" => Ok(None),
            "error" => Err(response
                .error
                .map(|error| error.message)
                .unwrap_or_else(|| "consent prompt failed".into())),
            status => Err(format!("consent prompt returned status {status}")),
        }
    }

    async fn invoke(&self, arguments: &str) -> Result<Vec<u8>, String> {
        let mut recipient = one_shot_server().await?;
        let mut child = prompt_command(&self.executable, &recipient.public_key_bundle(), arguments)
            .spawn()
            .map_err(|error| format!("failed to start the consent prompt: {error}"))?;

        // Held open so the child sees EOF, and exits, if this daemon dies.
        let _stdin = child
            .stdin
            .take()
            .ok_or("consent prompt stdin was not piped")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("consent prompt stdout was not piped")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("consent prompt stderr was not piped")?;

        let output = async {
            let (stdout, stderr, status) = tokio::join!(
                read_bounded(stdout, MAX_STDOUT_BYTES),
                read_bounded(stderr, MAX_STDERR_BYTES),
                child.wait(),
            );
            Ok::<_, String>((
                stdout.map_err(|error| format!("failed to read consent prompt output: {error}"))?,
                stderr.map_err(|error| format!("failed to read consent prompt errors: {error}"))?,
                status
                    .map_err(|error| format!("failed to wait for the consent prompt: {error}"))?,
            ))
        };
        let (stdout, stderr, status) = match tokio::time::timeout(PROMPT_TIMEOUT, output).await {
            Ok(result) => result?,
            Err(_) => {
                let _ = child.kill().await;
                return Err("consent prompt timed out".into());
            }
        };
        if !status.success() {
            let detail = String::from_utf8_lossy(&stderr);
            let detail = detail.trim();
            return Err(if detail.is_empty() {
                format!("consent prompt exited with {status}")
            } else {
                format!("consent prompt exited with {status}: {detail}")
            });
        }

        let frame: MessageFrame = serde_json::from_slice(stdout.trim_ascii())
            .map_err(|error| format!("consent prompt returned an invalid frame: {error}"))?;
        // The frame came off this child's own stdout pipe, so its sender key is
        // trusted for this one response and nothing else.
        recipient
            .add_runtime_approval(&frame.public_key)
            .map_err(|error| error.to_string())?;
        let captured = Arc::new(StdMutex::new(None));
        let output = captured.clone();
        recipient
            .handle_frame(&frame, move |plaintext| {
                *output.lock().expect("consent output lock poisoned") = Some(plaintext.to_vec());
                async { Ok::<Option<Vec<u8>>, String>(None) }
            })
            .await
            .map_err(|error| error.to_string())?;
        captured
            .lock()
            .map_err(|_| "consent output lock was poisoned".to_owned())?
            .take()
            .ok_or_else(|| "consent prompt returned an unauthenticated response".into())
    }
}

fn prompt_command(path: &Path, public_key: &str, arguments: &str) -> Command {
    let mut command = Command::new(path);
    command
        .arg("--public-key")
        .arg(public_key)
        .arg("passkey")
        .arg(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
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
            "consent prompt output exceeded the allowed size",
        ))
    } else {
        Ok(bytes)
    }
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
struct MemoryStore(StdMutex<Option<Vec<u8>>>);

impl StateStore for MemoryStore {
    fn load(&self) -> WireFuture<'_, keeless_lesswire::Result<Option<Vec<u8>>>> {
        Box::pin(async {
            Ok(self
                .0
                .lock()
                .map_err(|_| keeless_lesswire::Error::Host("consent state lock poisoned".into()))?
                .clone())
        })
    }

    fn save<'a>(&'a self, value: &'a [u8]) -> WireFuture<'a, keeless_lesswire::Result<()>> {
        Box::pin(async move {
            *self.0.lock().map_err(|_| {
                keeless_lesswire::Error::Host("consent state lock poisoned".into())
            })? = Some(value.to_vec());
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
#[serde(rename_all = "camelCase")]
struct PasskeyRequest<'a> {
    mode: &'static str,
    rp_id: &'a str,
    accounts: &'a [Account],
}

#[derive(Deserialize)]
struct PasskeyResponse<'a> {
    version: u8,
    kind: &'a str,
    status: &'a str,
    #[serde(default)]
    result: Option<PasskeyResult>,
    #[serde(default)]
    error: Option<PasskeyError>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PasskeyResult {
    account_id: String,
}

#[derive(Deserialize)]
struct PasskeyError {
    message: String,
}
