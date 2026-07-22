use std::{
    collections::HashMap,
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{self, Read},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use fs2::FileExt;
use keeless_core::{KeelessCore, KeelessHost, StorageProvider, SystemClock};
use keeless_host_desktop::{
    config::{CORE_SETTINGS_FILE, DesktopConfig, WIRE_STATE_FILE},
    ipc::{self, PickMode, Request, Response, ServerListener},
    storage::LocalFileStorage,
    ui::UiBridge,
};
use keeless_lesswire::{MessageFrame, PublicKeyBundle, Server, ServerHost};
use tokio::sync::Mutex;

#[derive(Debug, Eq, PartialEq)]
struct Arguments {
    approved_keys: Vec<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = parse_args(std::env::args_os().skip(1))?;
    let core_config = Arc::new(DesktopConfig::project(CORE_SETTINGS_FILE)?);
    let wire_config = Arc::new(DesktopConfig::project(WIRE_STATE_FILE)?);
    std::fs::create_dir_all(core_config.directory()?)?;
    protect_directory(core_config.directory()?)?;
    let _singleton = singleton_lock(core_config.directory()?)?;

    #[cfg(unix)]
    remove_stale_socket()?;

    let shutdown = Arc::new(AtomicBool::new(false));
    spawn_stdin_eof_monitor(shutdown.clone());
    let (ui, app) = UiBridge::channel(shutdown.clone());
    let background_shutdown = shutdown.clone();
    let background = std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
        let result = runtime.block_on(run_server(
            core_config,
            wire_config,
            ui,
            arguments.approved_keys,
            background_shutdown.clone(),
        ));
        background_shutdown.store(true, Ordering::Relaxed);
        result.map_err(|error| error.to_string())
    });

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Keeless daemon")
            .with_visible(false)
            .with_inner_size([520.0, 260.0]),
        ..Default::default()
    };
    let ui_result = eframe::run_native("Keeless daemon", options, Box::new(|_| Ok(Box::new(app))));
    shutdown.store(true, Ordering::Relaxed);
    let server_result = background
        .join()
        .map_err(|_| io::Error::other("daemon server thread panicked"))?;
    server_result.map_err(io::Error::other)?;
    ui_result?;

    #[cfg(unix)]
    let _ = std::fs::remove_file(ipc::endpoint_path());
    Ok(())
}

async fn run_server(
    core_config: Arc<DesktopConfig>,
    wire_config: Arc<DesktopConfig>,
    ui: Arc<UiBridge>,
    runtime_approved_clients: Vec<String>,
    shutdown: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let storage = Arc::new(LocalFileStorage::new());
    let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
    providers.insert("local-file".into(), storage.clone());
    let host = KeelessHost {
        storage_providers: providers,
        config_provider: core_config,
        password_input: Some(ui.clone()),
        clock: Arc::new(SystemClock),
    };
    let core = KeelessCore::new(host).await?;
    let server = Server::new(ServerHost {
        store: wire_config,
        approval_provider: ui,
        clock: Arc::new(keeless_lesswire::SystemClock),
        runtime_approved_clients,
    })
    .await?;
    let state = Arc::new(Mutex::new(DaemonState { core, server }));
    let mut listener = ServerListener::bind()?;
    let mut tick = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let mut connection = accepted?;
                let state = state.clone();
                let storage = storage.clone();
                tokio::spawn(async move {
                    let request = match connection.receive().await {
                        Ok(request) => request,
                        Err(_) => return,
                    };
                    let response = handle_request(request, state, storage).await;
                    let _ = connection.send(&response).await;
                });
            }
            _ = tick.tick() => {
                if let Ok(mut state) = state.try_lock() {
                    state.core.tick();
                }
                if shutdown.load(Ordering::Relaxed) {
                    return Ok(());
                }
            }
        }
    }
}

struct DaemonState {
    core: KeelessCore,
    server: Server,
}

async fn handle_request(
    request: Request,
    state: Arc<Mutex<DaemonState>>,
    storage: Arc<LocalFileStorage>,
) -> Response {
    match request {
        Request::Ping => Response::Pong,
        Request::HandleFrame(bytes) => {
            if bytes.len() > keeless_lesswire::MAX_FRAME_SIZE {
                return Response::Frame(None);
            }
            let Ok(frame) = serde_json::from_slice::<MessageFrame>(&bytes) else {
                return Response::Frame(None);
            };
            let mut state = state.lock().await;
            let DaemonState { core, server } = &mut *state;
            match server
                .handle_frame(&frame, |plaintext| async move {
                    core.handle_payload(&plaintext).await
                })
                .await
            {
                Ok(response) => {
                    Response::Frame(response.and_then(|frame| serde_json::to_vec(&frame).ok()))
                }
                Err(error) => Response::Error(error.to_string()),
            }
        }
        Request::PickLocalFile(mode) => {
            let dialog = rfd::AsyncFileDialog::new().add_filter("KeePass database", &["kdbx"]);
            let selected = match mode {
                PickMode::Open => dialog.pick_file().await,
                PickMode::Create => dialog.save_file().await,
            };
            match selected {
                None => Response::LocalFile(None),
                Some(file) => match storage.grant_picker_path(file.path().to_path_buf()) {
                    Ok(token) => Response::LocalFile(Some(token)),
                    Err(error) => Response::Error(error.to_string()),
                },
            }
        }
    }
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> io::Result<Arguments> {
    let mut args = args.into_iter();
    let mut approved_keys = Vec::new();
    while let Some(argument) = args.next() {
        if argument != "--approve-key" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown argument: {}", argument.to_string_lossy()),
            ));
        }
        let value = args.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "--approve-key requires a value",
            )
        })?;
        let value = value.into_string().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "--approve-key must be valid UTF-8",
            )
        })?;
        let bundle = PublicKeyBundle::parse(&value).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "--approve-key must be a canonical public key bundle",
            )
        })?;
        approved_keys.push(bundle.as_str().to_owned());
    }
    Ok(Arguments { approved_keys })
}

fn spawn_stdin_eof_monitor(shutdown: Arc<AtomicBool>) {
    std::thread::spawn(move || monitor_eof(io::stdin().lock(), &shutdown));
}

fn monitor_eof(mut input: impl Read, shutdown: &AtomicBool) {
    let mut buffer = [0_u8; 64];
    loop {
        match input.read(&mut buffer) {
            Ok(0) | Err(_) => {
                shutdown.store(true, Ordering::Relaxed);
                return;
            }
            Ok(_) => {}
        }
    }
}

fn singleton_lock(directory: &std::path::Path) -> io::Result<File> {
    let path = directory.join("daemon.lock");
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    FileExt::try_lock_exclusive(&file)
        .map_err(|_| io::Error::new(io::ErrorKind::AlreadyExists, "Keeless daemon is running"))?;
    Ok(file)
}

#[cfg(unix)]
fn protect_directory(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(windows)]
fn protect_directory(_: &std::path::Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn remove_stale_socket() -> io::Result<()> {
    match std::fs::remove_file(ipc::endpoint_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeless_lesswire::{ApprovalProvider, Client, Identity, StateStore, WireFuture};

    struct DenyApproval;

    impl ApprovalProvider for DenyApproval {
        fn approve(&self, _: &str) -> WireFuture<'_, keeless_lesswire::Result<bool>> {
            Box::pin(async { Ok(false) })
        }
    }

    fn bundle(seed: u8) -> String {
        Identity::from_secrets([seed; 32], [seed.wrapping_add(1); 32]).public_key_bundle()
    }

    #[test]
    fn parses_repeatable_runtime_approvals() {
        let first = bundle(1);
        let second = bundle(3);
        let parsed = parse_args([
            OsString::from("--approve-key"),
            OsString::from(&first),
            OsString::from("--approve-key"),
            OsString::from(&second),
        ])
        .unwrap();
        assert_eq!(parsed.approved_keys, vec![first, second]);
    }

    #[test]
    fn rejects_unknown_missing_and_invalid_arguments() {
        assert_eq!(parse_args([]).unwrap().approved_keys, Vec::<String>::new());
        assert!(parse_args([OsString::from("--unknown")]).is_err());
        assert!(parse_args([OsString::from("--approve-key")]).is_err());
        assert!(
            parse_args([
                OsString::from("--approve-key"),
                OsString::from("not-a-bundle")
            ])
            .is_err()
        );
    }

    #[test]
    fn stdin_eof_triggers_shutdown_after_ignoring_input() {
        let shutdown = AtomicBool::new(false);
        monitor_eof(&b"parent data"[..], &shutdown);
        assert!(shutdown.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn runtime_approval_is_not_persisted_or_reused_by_next_server() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(DesktopConfig::at(directory.path().join(WIRE_STATE_FILE)));
        let clock = Arc::new(keeless_lesswire::SystemClock);
        let identity = Identity::from_secrets([7; 32], [9; 32]);
        let runtime_bundle = identity.public_key_bundle();

        let server = Server::new(ServerHost {
            store: store.clone(),
            approval_provider: Arc::new(DenyApproval),
            clock: clock.clone(),
            runtime_approved_clients: vec![runtime_bundle.clone()],
        })
        .await
        .unwrap();
        drop(server);

        let persisted = StateStore::load(&*store).await.unwrap().unwrap();
        let persisted: serde_json::Value = serde_json::from_slice(&persisted).unwrap();
        assert!(
            !persisted["approvedClientBundles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == &runtime_bundle)
        );

        let mut restarted = Server::new(ServerHost {
            store,
            approval_provider: Arc::new(DenyApproval),
            clock: clock.clone(),
            runtime_approved_clients: Vec::new(),
        })
        .await
        .unwrap();
        let client = Client::new(identity, None, clock).unwrap();
        assert!(
            restarted
                .handle_frame(&client.handshake_frame().unwrap(), |_| async {
                    Ok::<_, ()>(None)
                })
                .await
                .unwrap()
                .is_none()
        );
    }
}
