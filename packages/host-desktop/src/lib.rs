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
    keeless_schema::{DatabaseNodeId, OperationOutcome, OperationResponse, OperationSuccess},
};
use keeless_host_desktop_shared::ipc;
use napi::{
    ValueType,
    bindgen_prelude::{
        Buffer, FromNapiValue, Function, JsObjectValue, Object, TypeName, ValidateNapiValue,
    },
    threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode},
};
use napi_derive::napi;
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};

use crate::{
    config::{CORE_STATE_FILE, DesktopConfig, WIRE_STATE_FILE},
    native_ui::NativeUi,
    persistence::DesktopDatabasePersistence,
    storage::{DesktopStorageConfigurer, LocalFileStorage},
};
use ipc::{Request, Response, ServerListener};

type EntryFocusCallback = ThreadsafeFunction<String, (), String, napi::Status, true>;
pub(crate) type NativeUiAnchorProvider =
    ThreadsafeFunction<(), Option<NativeUiAnchor>, (), napi::Status, true>;

struct HostState {
    inner: Mutex<InnerState>,
    storage: Arc<LocalFileStorage>,
    entry_focus: Arc<StdMutex<Option<EntryFocusCallback>>>,
}

struct InnerState {
    core: KeelessCore,
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

#[napi(object)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeUiAnchor {
    pub x: i32,
    pub y: i32,
}

pub struct DesktopHostOptions {
    native_ui_path: String,
    native_ui_anchor_provider: NativeUiAnchorProvider,
}

impl TypeName for DesktopHostOptions {
    fn type_name() -> &'static str {
        "Object"
    }

    fn value_type() -> ValueType {
        ValueType::Object
    }
}

impl ValidateNapiValue for DesktopHostOptions {}

impl FromNapiValue for DesktopHostOptions {
    unsafe fn from_napi_value(
        env: napi::sys::napi_env,
        value: napi::sys::napi_value,
    ) -> napi::Result<Self> {
        let options = Object::from_raw(env, value);
        let native_ui_path = options.get_named_property("nativeUiPath")?;
        let native_ui_anchor: Function<'_, (), Option<NativeUiAnchor>> =
            options.get_named_property("getNativeUiAnchor")?;
        let native_ui_anchor_provider = native_ui_anchor
            .build_threadsafe_function()
            .callee_handled::<true>()
            .build()?;
        Ok(Self {
            native_ui_path,
            native_ui_anchor_provider,
        })
    }
}

#[napi]
pub struct DesktopHost {
    runtime: Arc<HostRuntime>,
}

#[napi]
impl DesktopHost {
    #[napi(
        factory,
        ts_args_type = "options: { nativeUiPath: string; getNativeUiAnchor: () => NativeUiAnchor | undefined }"
    )]
    pub async fn create(options: DesktopHostOptions) -> napi::Result<Self> {
        let DesktopHostOptions {
            native_ui_path,
            native_ui_anchor_provider,
        } = options;
        let native_ui_path = PathBuf::from(native_ui_path);
        if !native_ui_path.is_absolute() {
            return Err(napi_error("nativeUiPath must be absolute"));
        }
        let metadata = std::fs::metadata(&native_ui_path)
            .map_err(|error| napi_error(format!("native UI executable is unavailable: {error}")))?;
        if !metadata.is_file() {
            return Err(napi_error("native UI path is not a file"));
        }

        let wire_config = Arc::new(
            DesktopConfig::project(WIRE_STATE_FILE)
                .map_err(|error| napi_error(error.to_string()))?,
        );
        let core_state = Arc::new(
            DesktopConfig::project(CORE_STATE_FILE)
                .map_err(|error| napi_error(error.to_string()))?,
        );
        let (shutdown, shutdown_rx) = watch::channel(false);
        let native_ui = Arc::new(NativeUi::new(
            native_ui_path,
            native_ui_anchor_provider,
            shutdown_rx,
        ));
        let storage = Arc::new(LocalFileStorage::new());
        let database_persistence = Arc::new(
            DesktopDatabasePersistence::project(storage.clone())
                .map_err(|error| napi_error(error.to_string()))?,
        );
        let mut providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
        providers.insert("local-file".into(), storage.clone());
        let core = KeelessCore::new(KeelessHost {
            storage_providers: providers,
            untrusted_state: wire_config,
            core_state,
            connection_approval: native_ui.clone(),
            password_input: Some(native_ui.clone()),
            passkey_consent: Some(native_ui.clone()),
            clock: Arc::new(SystemClock),
            database_persistence,
            storage_configurer: Some(Arc::new(DesktopStorageConfigurer::new(storage.clone()))),
            task_spawner: Some(Arc::new(TokioTaskSpawner)),
            transfer_provider: None,
        })
        .await
        .map_err(|error| napi_error(error.to_string()))?;
        let state = Arc::new(HostState {
            inner: Mutex::new(InnerState { core }),
            storage,
            entry_focus: Arc::new(StdMutex::new(None)),
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

    #[napi(js_name = "addRuntimeClient")]
    pub async fn add_runtime_client(&self, bundle: String) -> napi::Result<String> {
        self.ensure_open()?;
        let mut state = self.runtime.state.inner.lock().await;
        state.core.add_runtime_client(&bundle).map_err(napi_error)?;
        Ok(state.core.untrusted_public_key_bundle())
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

    #[napi(js_name = "onEntryFocus")]
    pub fn on_entry_focus(&self, callback: Function<'_, String, ()>) -> napi::Result<()> {
        self.ensure_open()?;
        let callback = callback
            .build_threadsafe_function()
            .callee_handled::<true>()
            .build()?;
        *self
            .runtime
            .state
            .entry_focus
            .lock()
            .map_err(|_| napi_error("entry focus callback lock was poisoned"))? = Some(callback);
        Ok(())
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
    let entry_focus = state.entry_focus.clone();
    let mut inner = state.inner.lock().await;
    let response = inner
        .core
        .handle_frame(bytes)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(entry_id) = entry_focus_id(response.as_deref()) {
        notify_entry_focus(&entry_focus, entry_id);
    }
    Ok(response)
}

fn entry_focus_id(response: Option<&[u8]>) -> Option<String> {
    let response = serde_json::from_slice::<OperationResponse>(response?).ok()?;
    let OperationOutcome::Success {
        success: OperationSuccess::RegisterPasskey(result),
    } = response.outcome
    else {
        return None;
    };
    Some(match result.entry_id {
        DatabaseNodeId::Uuid(value) => value,
        DatabaseNodeId::Int(value) => value.to_string(),
    })
}

fn notify_entry_focus(entry_focus: &StdMutex<Option<EntryFocusCallback>>, entry_id: String) {
    let Ok(callback) = entry_focus.lock() else {
        return;
    };
    let Some(callback) = callback.as_ref() else {
        return;
    };
    let _ = callback.call(Ok(entry_id), ThreadsafeFunctionCallMode::NonBlocking);
}

async fn handle_request(request: Request, state: &HostState) -> Response {
    match request {
        Request::Ping => Response::Pong,
        Request::Bootstrap => {
            Response::Bootstrap(state.inner.lock().await.core.untrusted_public_key_bundle())
        }
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
                        tokio::select! {
                            response = handle_request(request, &state) => {
                                let _ = connection.send(&response).await;
                            }
                            _ = connection.wait_for_disconnect() => {
                                // Dropping the request future also drops a native-ui child
                                // spawned for it, taking down a CTAP ceremony on CANCEL.
                            }
                        }
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

#[cfg(test)]
mod tests {
    use super::entry_focus_id;

    #[test]
    fn extracts_focus_id_from_passkey_registration() {
        let response = br#"{
            "requestId": "request",
            "status": "success",
            "op": "registerPasskey",
            "result": {
                "entryId": "entry",
                "credentialId": "credential",
                "authenticatorData": "authenticator"
            }
        }"#;

        assert_eq!(entry_focus_id(Some(response)), Some("entry".into()));
    }

    #[test]
    fn ignores_other_responses() {
        let response = br#"{
            "requestId": "request",
            "status": "error",
            "error": { "code": "denied", "message": "Denied" }
        }"#;

        assert_eq!(entry_focus_id(Some(response)), None);
    }
}
