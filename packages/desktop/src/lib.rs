use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use keeless_host_desktop::ipc::{Client, PickMode as IpcPickMode};
use keeless_schema::MessageFrame;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tokio::{sync::Mutex, time::sleep};

const CONNECT_ATTEMPTS: usize = 30;
const CONNECT_DELAY: Duration = Duration::from_millis(100);

struct DesktopState {
    startup: Mutex<()>,
    has_connected: AtomicBool,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum EnsureDaemonResult {
    Connected,
    Started,
    Restarted,
}

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum PickMode {
    Open,
    Create,
}

fn daemon_name() -> &'static str {
    if cfg!(windows) {
        "keeless-daemon.exe"
    } else {
        "keeless-daemon"
    }
}

fn daemon_path(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("KEELESS_DAEMON_PATH") {
        return Ok(path.into());
    }

    let current_exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let packaged = current_exe
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(daemon_name());
    if packaged.is_file() {
        return Ok(packaged);
    }

    app.path()
        .resource_dir()
        .map(|directory| directory.join(daemon_name()))
        .map_err(|error| error.to_string())
}

fn start_daemon(app: &AppHandle) -> Result<(), String> {
    Command::new(daemon_path(app)?)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to start keeless-daemon: {error}"))
}

#[tauri::command]
async fn ensure_daemon(
    app: AppHandle,
    state: State<'_, DesktopState>,
    _default_approved_bundle: String,
) -> Result<EnsureDaemonResult, String> {
    let _startup = state.startup.lock().await;
    println!("[Keeless] Loading daemon...");

    if Client::ping().await.is_ok() {
        println!("[Keeless] Found living daemon.");
        state.has_connected.store(true, Ordering::Release);
        return Ok(EnsureDaemonResult::Connected);
    }

    // Another desktop process may win the startup race, so a spawn error is not
    // final until all connection retries have also failed.
    let spawn_error = start_daemon(&app).err();
    println!("[Keeless] Starting daemon...");

    let mut last_error = None;
    for _ in 0..CONNECT_ATTEMPTS {
        sleep(CONNECT_DELAY).await;
        match Client::ping().await {
            Ok(()) => {
                let was_connected = state.has_connected.swap(true, Ordering::AcqRel);
                println!("[Keeless] Started daemon.");

                return Ok(if was_connected {
                    EnsureDaemonResult::Restarted
                } else {
                    EnsureDaemonResult::Started
                });
            }
            Err(error) => last_error = Some(error.to_string()),
        }
    }

    println!("[Keeless] Failed to start daemon.");
    if let Some(ref err) = spawn_error {
      println!("{}", err);
    }

    if let Some(ref err) = last_error {
      println!("{}", err);
    }

    Err(spawn_error
        .or(last_error)
        .unwrap_or_else(|| "daemon did not become available".to_owned()))
}

#[tauri::command]
async fn relay_frame(frame: MessageFrame) -> Result<Option<MessageFrame>, String> {
    let bytes = serde_json::to_vec(&frame).map_err(|error| error.to_string())?;
    Client::handle_frame(bytes)
        .await
        .map_err(|error| error.to_string())?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()))
        .transpose()
}

#[tauri::command]
async fn pick_local_file(mode: PickMode) -> Result<Option<String>, String> {
    Client::pick_local_file(match mode {
        PickMode::Open => IpcPickMode::Open,
        PickMode::Create => IpcPickMode::Create,
    })
    .await
    .map_err(|error| error.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .manage(DesktopState {
            startup: Mutex::new(()),
            has_connected: AtomicBool::new(false),
        })
        .invoke_handler(tauri::generate_handler![
            ensure_daemon,
            relay_frame,
            pick_local_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running keeless desktop");
}
