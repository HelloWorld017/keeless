use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use fs2::FileExt;
use keeless_core::{KeelessCore, KeelessHost, StorageProvider, SystemClock};
use keeless_host_desktop::{
    config::DesktopConfig,
    ipc::{self, PickMode, Request, Response, ServerListener},
    storage::LocalFileStorage,
    ui::UiBridge,
};
use tokio::sync::Mutex;

const IDLE_EXIT: Duration = Duration::from_secs(15 * 60);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Arc::new(DesktopConfig::project()?);
    std::fs::create_dir_all(config.directory()?)?;
    protect_directory(config.directory()?)?;
    let _singleton = singleton_lock(config.directory()?)?;

    #[cfg(unix)]
    remove_stale_socket()?;

    let shutdown = Arc::new(AtomicBool::new(false));
    let (ui, app) = UiBridge::channel(shutdown.clone());
    let background_shutdown = shutdown.clone();
    let background = std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
        let result = runtime.block_on(run_server(config, ui, background_shutdown.clone()));
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
    config: Arc<DesktopConfig>,
    ui: Arc<UiBridge>,
    shutdown: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let storage = Arc::new(LocalFileStorage::new());
    let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
    providers.insert("local-file".into(), storage.clone());
    let host = KeelessHost {
        default_approved_keys: Vec::new(),
        storage_providers: providers,
        config_provider: config,
        approval_provider: ui.clone(),
        password_input: Some(ui),
        clock: Arc::new(SystemClock),
    };
    let core = Arc::new(Mutex::new(KeelessCore::new(host).await?));
    let mut listener = ServerListener::bind()?;
    let started = Instant::now();
    let last_request_ms = Arc::new(AtomicU64::new(0));
    let active_requests = Arc::new(AtomicUsize::new(0));
    let mut tick = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let mut connection = accepted?;
                let core = core.clone();
                let storage = storage.clone();
                let activity = last_request_ms.clone();
                let active = active_requests.clone();
                tokio::spawn(async move {
                    let request = match connection.receive().await {
                        Ok(request) => request,
                        Err(_) => return,
                    };
                    active.fetch_add(1, Ordering::Relaxed);
                    activity.store(elapsed_ms(started), Ordering::Relaxed);
                    let response = handle_request(request, core, storage).await;
                    let _ = connection.send(&response).await;
                    activity.store(elapsed_ms(started), Ordering::Relaxed);
                    active.fetch_sub(1, Ordering::Relaxed);
                });
            }
            _ = tick.tick() => {
                if let Ok(mut core) = core.try_lock() {
                    core.tick();
                }
                let idle_ms = elapsed_ms(started).saturating_sub(last_request_ms.load(Ordering::Relaxed));
                if shutdown.load(Ordering::Relaxed)
                    || (active_requests.load(Ordering::Relaxed) == 0
                        && idle_ms >= IDLE_EXIT.as_millis() as u64)
                {
                    return Ok(());
                }
            }
        }
    }
}

async fn handle_request(
    request: Request,
    core: Arc<Mutex<KeelessCore>>,
    storage: Arc<LocalFileStorage>,
) -> Response {
    match request {
        Request::Ping => Response::Pong,
        Request::HandleFrame(frame) => match core.lock().await.handle(&frame).await {
            Ok(frame) => Response::Frame(frame),
            Err(error) => Response::Error(error.to_string()),
        },
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

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
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
