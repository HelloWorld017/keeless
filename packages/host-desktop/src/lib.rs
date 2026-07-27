pub mod config;
mod native_ui;
pub mod persistence;
pub mod storage;

use std::{
    collections::HashMap,
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use keeless_core::{
    HostFuture, KeelessCore, KeelessHost, StorageProvider, SystemClock, TaskSpawner,
};
use keeless_host_desktop_shared::ipc;
use keeless_lesswire::{MessageFrame, Server, ServerHost};
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};

use crate::{
    config::{CORE_SETTINGS_FILE, DesktopConfig, WIRE_STATE_FILE},
    native_ui::NativeUi,
    persistence::DesktopDatabasePersistence,
    storage::LocalFileStorage,
};
use ipc::{Request, Response, ServerListener};

struct HostState {
    inner: Mutex<InnerState>,
    storage: Arc<LocalFileStorage>,
}

struct InnerState {
    core: KeelessCore,
    server: Server,
}

#[derive(Debug)]
struct TokioTaskSpawner;

impl TaskSpawner for TokioTaskSpawner {
    fn spawn(&self, task: HostFuture<'static, ()>) {
        tokio::spawn(task);
    }
}

struct HostRuntime {
    state: Arc<HostState>,
    shutdown: watch::Sender<bool>,
    tasks: StdMutex<Vec<JoinHandle<()>>>,
    closed: AtomicBool,
}

#[napi]
pub struct DesktopHost {
    runtime: Arc<HostRuntime>,
}

#[napi]
impl DesktopHost {
    #[napi(factory)]
    pub async fn create(native_ui_path: String) -> napi::Result<Self> {
        let native_ui_path = PathBuf::from(native_ui_path);
        if !native_ui_path.is_absolute() {
            return Err(napi_error("nativeUiPath must be absolute"));
        }
        let metadata = std::fs::metadata(&native_ui_path)
            .map_err(|error| napi_error(format!("native UI executable is unavailable: {error}")))?;
        if !metadata.is_file() {
            return Err(napi_error("native UI path is not a file"));
        }

        let core_config = Arc::new(
            DesktopConfig::project(CORE_SETTINGS_FILE)
                .map_err(|error| napi_error(error.to_string()))?,
        );
        let wire_config = Arc::new(
            DesktopConfig::project(WIRE_STATE_FILE)
                .map_err(|error| napi_error(error.to_string()))?,
        );
        let (shutdown, shutdown_rx) = watch::channel(false);
        let native_ui = Arc::new(NativeUi::new(native_ui_path, shutdown_rx));
        let storage = Arc::new(LocalFileStorage::new());
        let database_persistence = Arc::new(
            DesktopDatabasePersistence::project(storage.clone())
                .map_err(|error| napi_error(error.to_string()))?,
        );
        let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
        providers.insert("local-file".into(), storage.clone());
        let core = KeelessCore::new(KeelessHost {
            storage_providers: providers,
            config_provider: core_config,
            password_input: Some(native_ui.clone()),
            clock: Arc::new(SystemClock),
            database_persistence: Some(database_persistence),
            task_spawner: Some(Arc::new(TokioTaskSpawner)),
        })
        .await
        .map_err(|error| napi_error(error.to_string()))?;
        let server = Server::new(ServerHost {
            store: wire_config,
            approval_provider: native_ui.clone(),
            clock: Arc::new(keeless_lesswire::SystemClock),
            runtime_approved_clients: Vec::new(),
        })
        .await
        .map_err(|error| napi_error(error.to_string()))?;
        let state = Arc::new(HostState {
            inner: Mutex::new(InnerState { core, server }),
            storage,
        });
        let listener = bind_listener().map_err(|error| napi_error(error.to_string()))?;
        let tasks = vec![
            tokio::spawn(run_tick(state.clone(), shutdown.subscribe())),
            tokio::spawn(run_ipc(state.clone(), listener, shutdown.subscribe())),
        ];
        Ok(Self {
            runtime: Arc::new(HostRuntime {
                state,
                shutdown,
                tasks: StdMutex::new(tasks),
                closed: AtomicBool::new(false),
            }),
        })
    }

    #[napi(js_name = "registerClient")]
    pub async fn register_client(&self, bundle: String) -> napi::Result<()> {
        self.ensure_open()?;
        self.runtime
            .state
            .inner
            .lock()
            .await
            .server
            .add_runtime_approval(&bundle)
            .map_err(|error| napi_error(error.to_string()))
    }

    #[napi(js_name = "handleFrame")]
    pub async fn handle_frame(&self, frame: Buffer) -> napi::Result<Option<Buffer>> {
        self.ensure_open()?;
        handle_frame(&self.runtime.state, frame.as_ref())
            .await
            .map(|value| value.map(Buffer::from))
            .map_err(napi_error)
    }

    #[napi(js_name = "grantLocalFile")]
    pub fn grant_local_file(&self, path: String) -> napi::Result<String> {
        self.ensure_open()?;
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(napi_error("local file path must be absolute"));
        }
        self.runtime
            .state
            .storage
            .grant_picker_path(path)
            .map_err(napi_error)
    }

    #[napi]
    pub async fn shutdown(&self) -> napi::Result<()> {
        if self.runtime.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let _ = self.runtime.shutdown.send(true);
        let tasks = self
            .runtime
            .tasks
            .lock()
            .map_err(|_| napi_error("host task lock was poisoned"))?
            .drain(..)
            .collect::<Vec<_>>();
        for task in tasks {
            let _ = task.await;
        }
        #[cfg(unix)]
        let _ = std::fs::remove_file(ipc::endpoint_path());
        Ok(())
    }
}

impl DesktopHost {
    fn ensure_open(&self) -> napi::Result<()> {
        if self.runtime.closed.load(Ordering::Acquire) {
            Err(napi_error("desktop host is shut down"))
        } else {
            Ok(())
        }
    }
}

async fn handle_frame(state: &HostState, bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    if bytes.len() > keeless_lesswire::MAX_FRAME_SIZE {
        return Ok(None);
    }
    let Ok(frame) = serde_json::from_slice::<MessageFrame>(bytes) else {
        return Ok(None);
    };
    let mut inner = state.inner.lock().await;
    let InnerState { core, server } = &mut *inner;
    server
        .handle_frame(&frame, |plaintext| async move {
            core.handle_payload(&plaintext).await
        })
        .await
        .map_err(|error| error.to_string())?
        .map(|response| serde_json::to_vec(&response).map_err(|error| error.to_string()))
        .transpose()
}

async fn handle_request(request: Request, state: &HostState) -> Response {
    match request {
        Request::Ping => Response::Pong,
        Request::HandleFrame(bytes) => match handle_frame(state, &bytes).await {
            Ok(frame) => Response::Frame(frame),
            Err(error) => Response::Error(error),
        },
    }
}

async fn run_tick(state: Arc<HostState>, mut shutdown: watch::Receiver<bool>) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if let Ok(mut state) = state.inner.try_lock() {
                    state.core.tick().await;
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
        }
    }
}

async fn run_ipc(
    state: Arc<HostState>,
    mut listener: ServerListener,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(mut connection) => {
                    let state = state.clone();
                    connections.spawn(async move {
                        let Ok(request) = connection.receive().await else { return };
                        let response = handle_request(request, &state).await;
                        let _ = connection.send(&response).await;
                    });
                }
                Err(error) => {
                    eprintln!("desktop IPC listener failed: {error}");
                    return;
                }
            },
            _ = connections.join_next(), if !connections.is_empty() => {}
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    connections.abort_all();
                    while connections.join_next().await.is_some() {}
                    return;
                }
            }
        }
    }
}

fn bind_listener() -> ipc::Result<ServerListener> {
    #[cfg(unix)]
    {
        match std::fs::remove_file(ipc::endpoint_path()) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    ServerListener::bind()
}

fn napi_error(error: impl ToString) -> napi::Error {
    napi::Error::from_reason(error.to_string())
}
