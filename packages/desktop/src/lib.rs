use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use keeless_host_desktop::{
    config::{DESKTOP_WIRE_STATE_FILE, DesktopConfig},
    ipc::{Client as IpcClient, PickMode as IpcPickMode},
};
use keeless_lesswire::{
    ApprovalProvider, Client, Identity, MessageFrame, Server, ServerHost, SystemClock, WireFuture,
};
use serde::Deserialize;
use tauri::{
    AppHandle, Manager, RunEvent, State, WebviewWindowBuilder, WindowEvent,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
use tokio::{sync::Mutex, time::sleep};

const MAIN_WINDOW_LABEL: &str = "main";
const CONNECT_ATTEMPTS: usize = 50;
const CONNECT_DELAY: Duration = Duration::from_millis(100);

struct DenyApproval;

impl ApprovalProvider for DenyApproval {
    fn approve(&self, _: &str) -> WireFuture<'_, keeless_lesswire::Result<bool>> {
        Box::pin(async { Ok(false) })
    }
}

struct WireState {
    ui_server: Server,
    daemon_client: Client,
}

struct DesktopState {
    wire: Mutex<WireState>,
    daemon_stdin: StdMutex<Option<ChildStdin>>,
    stopping: AtomicBool,
}

struct WindowState {
    window_creation: StdMutex<()>,
    ready: AtomicBool,
    pending_open: AtomicBool,
}

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum PickMode {
    Open,
    Create,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorrelatedPayload {
    request_id: String,
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

fn daemon_command(path: &Path, approved_bundle: &str) -> Command {
    let mut command = Command::new(path);
    command
        .arg("--approve-key")
        .arg(approved_bundle)
        .stdin(Stdio::piped());
    command
}

fn process_is_alive(child: &mut Child) -> Result<(), String> {
    match child.try_wait() {
        Ok(None) => Ok(()),
        Ok(Some(status)) => Err(format!("keeless-daemon exited during startup: {status}")),
        Err(error) => Err(format!("failed to inspect keeless-daemon: {error}")),
    }
}

async fn connect_owned_daemon(child: &mut Child, client: &mut Client) -> Result<(), String> {
    // Give a daemon rejected by its singleton lock time to exit before probing shared IPC.
    sleep(CONNECT_DELAY).await;
    let mut last_error = "daemon IPC did not become available".to_owned();
    for _ in 0..CONNECT_ATTEMPTS {
        process_is_alive(child)?;
        match IpcClient::ping().await {
            Ok(()) => {
                process_is_alive(child)?;
                let handshake = serde_json::to_vec(&client.handshake_frame().map_err(wire_error)?)
                    .map_err(|error| error.to_string())?;
                match IpcClient::handle_frame(handshake).await {
                    Ok(Some(response)) => {
                        process_is_alive(child)?;
                        let response: MessageFrame =
                            serde_json::from_slice(&response).map_err(|error| error.to_string())?;
                        client.accept_handshake(&response).map_err(wire_error)?;
                        return Ok(());
                    }
                    Ok(None) => last_error = "daemon rejected startup handshake".into(),
                    Err(error) => last_error = error.to_string(),
                }
            }
            Err(error) => last_error = error.to_string(),
        }
        sleep(CONNECT_DELAY).await;
    }
    Err(last_error)
}

fn wire_error(error: keeless_lesswire::Error) -> String {
    error.to_string()
}

fn request_id(bytes: &[u8]) -> Result<String, String> {
    let payload: CorrelatedPayload = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid correlated daemon payload: {error}"))?;
    if payload.request_id.is_empty() {
        Err("daemon payload has an empty requestId".into())
    } else {
        Ok(payload.request_id)
    }
}

async fn initialize(app: &AppHandle) -> Result<(WireState, Child, ChildStdin), String> {
    let ui_store = Arc::new(
        DesktopConfig::project(DESKTOP_WIRE_STATE_FILE).map_err(|error| error.to_string())?,
    );
    let ui_server = Server::new(ServerHost {
        store: ui_store,
        approval_provider: Arc::new(DenyApproval),
        clock: Arc::new(SystemClock),
        runtime_approved_clients: Vec::new(),
    })
    .await
    .map_err(wire_error)?;

    let mut daemon_client = Client::new(
        Identity::generate().map_err(wire_error)?,
        None,
        Arc::new(SystemClock),
    )
    .map_err(wire_error)?;
    let mut child = daemon_command(&daemon_path(app)?, &daemon_client.public_key_bundle())
        .spawn()
        .map_err(|error| format!("failed to start keeless-daemon: {error}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "keeless-daemon stdin was not piped".to_owned())?;
    connect_owned_daemon(&mut child, &mut daemon_client).await?;

    Ok((
        WireState {
            ui_server,
            daemon_client,
        },
        child,
        stdin,
    ))
}

fn show_main_window(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<WindowState>();
    state.pending_open.store(true, Ordering::Release);
    if !state.ready.load(Ordering::Acquire) {
        return Ok(());
    }
    let _creation = state
        .window_creation
        .lock()
        .map_err(|_| "main window creation lock was poisoned".to_owned())?;
    state.pending_open.store(false, Ordering::Release);
    let window = if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        window
    } else {
        let config = app
            .config()
            .app
            .windows
            .iter()
            .find(|window| window.label == MAIN_WINDOW_LABEL)
            .ok_or_else(|| "main window configuration is missing".to_owned())?;
        let window = WebviewWindowBuilder::from_config(app, config)
            .map_err(|error| error.to_string())?
            .build()
            .map_err(|error| error.to_string())?;
        let close_window = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = close_window.hide();
            }
        });
        window
    };
    window.show().map_err(|error| error.to_string())?;
    window.unminimize().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

fn stop(app: &AppHandle) {
    let state = app.state::<DesktopState>();
    state.stopping.store(true, Ordering::Release);
    if let Ok(mut stdin) = state.daemon_stdin.lock() {
        stdin.take();
    }
}

fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Err(error) = show_main_window(app) {
                    eprintln!("failed to open main window: {error}");
                }
            }
            "quit" => {
                stop(app);
                app.exit(0);
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn watch_daemon(app: AppHandle, mut child: Child) {
    std::thread::spawn(move || {
        let status = child.wait();
        let state = app.state::<DesktopState>();
        if !state.stopping.load(Ordering::Acquire) {
            match status {
                Ok(status) => eprintln!("keeless-daemon exited unexpectedly: {status}"),
                Err(error) => eprintln!("failed to watch keeless-daemon: {error}"),
            }
            app.exit(1);
        }
    });
}

fn is_minimized(args: impl IntoIterator<Item = OsString>) -> bool {
    args.into_iter()
        .any(|argument| argument == OsStr::new("--minimized"))
}

#[tauri::command]
async fn register_client(state: State<'_, DesktopState>, bundle: String) -> Result<(), String> {
    state
        .wire
        .lock()
        .await
        .ui_server
        .add_runtime_approval(&bundle)
        .map_err(wire_error)
}

#[tauri::command]
async fn relay_frame(
    state: State<'_, DesktopState>,
    frame: MessageFrame,
) -> Result<Option<MessageFrame>, String> {
    let mut wire = state.wire.lock().await;
    let WireState {
        ui_server,
        daemon_client,
    } = &mut *wire;
    ui_server
        .handle_frame(&frame, |plaintext| async move {
            let expected_id = request_id(&plaintext)?;
            let daemon_frame = daemon_client.encrypt(&plaintext).map_err(wire_error)?;
            let bytes = serde_json::to_vec(&daemon_frame).map_err(|error| error.to_string())?;
            let Some(response) = IpcClient::handle_frame(bytes)
                .await
                .map_err(|error| error.to_string())?
            else {
                return Ok(None);
            };
            let response: MessageFrame =
                serde_json::from_slice(&response).map_err(|error| error.to_string())?;
            let plaintext = daemon_client
                .decrypt(&response)
                .map_err(wire_error)?
                .ok_or_else(|| "daemon returned an invalid encrypted response".to_owned())?;
            if request_id(&plaintext)? != expected_id {
                return Err::<Option<Vec<u8>>, String>(
                    "daemon returned a mismatched requestId".into(),
                );
            }
            Ok(Some(plaintext.to_vec()))
        })
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn pick_local_file(mode: PickMode) -> Result<Option<String>, String> {
    IpcClient::pick_local_file(match mode {
        PickMode::Open => IpcPickMode::Open,
        PickMode::Create => IpcPickMode::Create,
    })
    .await
    .map_err(|error| error.to_string())
}

pub fn run() {
    let minimized = is_minimized(std::env::args_os().skip(1));
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Err(error) = show_main_window(app) {
                eprintln!("failed to open main window for secondary instance: {error}");
            }
        }))
        .manage(WindowState {
            window_creation: StdMutex::new(()),
            ready: AtomicBool::new(false),
            pending_open: AtomicBool::new(false),
        })
        .invoke_handler(tauri::generate_handler![
            register_client,
            relay_frame,
            pick_local_file
        ])
        .setup(move |app| {
            let (wire, child, stdin) = tauri::async_runtime::block_on(initialize(app.handle()))
                .map_err(std::io::Error::other)?;
            app.manage(DesktopState {
                wire: Mutex::new(wire),
                daemon_stdin: StdMutex::new(Some(stdin)),
                stopping: AtomicBool::new(false),
            });
            watch_daemon(app.handle().clone(), child);
            create_tray(app.handle())?;
            let windows = app.state::<WindowState>();
            windows.ready.store(true, Ordering::Release);
            if !minimized || windows.pending_open.swap(false, Ordering::AcqRel) {
                show_main_window(app.handle()).map_err(std::io::Error::other)?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building keeless desktop");

    app.run(|app, event| {
        if matches!(event, RunEvent::ExitRequested { .. }) {
            stop(app);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_only_exact_minimized_argument() {
        assert!(is_minimized([OsString::from("--minimized")]));
        assert!(is_minimized([
            OsString::from("--other"),
            OsString::from("--minimized")
        ]));
        assert!(!is_minimized([OsString::from("--minimized=true")]));
        assert!(!is_minimized([]));
    }

    #[test]
    fn daemon_command_has_runtime_approval_and_piped_stdin() {
        let command = daemon_command(Path::new("daemon"), "bundle");
        assert_eq!(command.get_program(), OsStr::new("daemon"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [OsStr::new("--approve-key"), OsStr::new("bundle")]
        );
    }
}
