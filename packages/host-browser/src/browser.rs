use std::{collections::HashMap, rc::Rc, sync::Arc};

use futures::lock::Mutex;
use gloo_timers::future::TimeoutFuture;
use keeless_core::{
    ConnectionApprovalProvider, ConnectionApprovalRequest, HostFuture, KeelessCore, KeelessHost,
    StorageProvider, TaskSpawner,
};
use keeless_sync::{WebDavAuth, WebDavProvider};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
use wasm_bindgen_futures::spawn_local;
use web_sys::{File, FileSystemFileHandle};

use crate::{
    clock::BrowserClock,
    config::BrowserConfig,
    persistence::{BrowserDatabasePersistence, BrowserStorageConfigurer},
    storages::{indexeddb::IndexedDbStorage, local_file::LocalFileStorage},
    utils::indexeddb::{CORE_CONFIG_KEY, IndexedDb, WIRE_CONFIG_KEY, js_error},
};

struct BrowserApproval;

struct BrowserTaskSpawner;

impl TaskSpawner for BrowserTaskSpawner {
    fn spawn(&self, task: HostFuture<'static, ()>) {
        spawn_local(task);
    }
}

impl ConnectionApprovalProvider for BrowserApproval {
    fn approve_connection(
        &self,
        request: ConnectionApprovalRequest,
    ) -> HostFuture<'_, keeless_core::Result<bool>> {
        Box::pin(async move {
            let message = match request.kind {
                keeless_core::ConnectionApprovalKind::Initial => format!(
                    "Allow a limited Keeless connection from a {} client?\n\nClient: {}\nServer: {}",
                    scope_name(request.sender_scope),
                    request.sender,
                    request.recipient,
                ),
                keeless_core::ConnectionApprovalKind::Upgrade => format!(
                    "Allow this {} client to access the selected database?\n\nClient: {}\nDatabase server: {}",
                    scope_name(request.sender_scope),
                    request.sender,
                    request.recipient,
                ),
            };
            web_sys::window()
                .ok_or_else(|| {
                    keeless_core::CoreError::Host("browser window is unavailable".into())
                })?
                .confirm_with_message(&message)
                .map_err(|error| keeless_core::CoreError::Host(format!("{error:?}")))
        })
    }
}

fn scope_name(scope: keeless_lesswire::KeyScope) -> &'static str {
    match scope {
        keeless_lesswire::KeyScope::CoreUntrusted => "core_untrusted",
        keeless_lesswire::KeyScope::Core => "core",
        keeless_lesswire::KeyScope::App => "app",
        keeless_lesswire::KeyScope::Passkey => "passkey",
        keeless_lesswire::KeyScope::NativeUi => "native_ui",
    }
}

struct BrowserState {
    core: KeelessCore,
}

#[wasm_bindgen]
pub struct BrowserCore {
    state: Rc<Mutex<BrowserState>>,
}

#[wasm_bindgen]
impl BrowserCore {
    #[wasm_bindgen(js_name = create)]
    // KeelessHost intentionally uses Arc for one API across native and single-threaded WASM.
    #[allow(clippy::arc_with_non_send_sync)]
    pub async fn create() -> Result<BrowserCore, JsValue> {
        let idb = IndexedDb::open().await?;
        let mut storage_providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
        storage_providers.insert(
            "indexeddb".into(),
            Arc::new(IndexedDbStorage { idb: idb.clone() }),
        );
        let host = KeelessHost {
            storage_providers,
            untrusted_state: Arc::new(BrowserConfig {
                idb: idb.clone(),
                key: WIRE_CONFIG_KEY,
            }),
            core_state: Arc::new(BrowserConfig {
                idb: idb.clone(),
                key: CORE_CONFIG_KEY,
            }),
            connection_approval: Arc::new(BrowserApproval),
            password_input: None,
            passkey_consent: None,
            clock: Arc::new(BrowserClock),
            database_persistence: Arc::new(BrowserDatabasePersistence::new(idb.clone())),
            storage_configurer: Some(Arc::new(BrowserStorageConfigurer::new(idb.clone()))),
            task_spawner: Some(Arc::new(BrowserTaskSpawner)),
            transfer_provider: None,
        };
        let core = KeelessCore::new(host).await.map_err(js_error)?;
        let state = Rc::new(Mutex::new(BrowserState { core }));
        let tick_state = Rc::downgrade(&state);
        spawn_local(async move {
            loop {
                TimeoutFuture::new(1_000).await;
                let Some(tick_state) = tick_state.upgrade() else {
                    return;
                };
                if let Some(mut state) = tick_state.try_lock() {
                    state.core.tick().await;
                }
            }
        });
        Ok(Self { state })
    }

    #[wasm_bindgen(js_name = connect)]
    pub async fn connect(&self, client_bundle: String) -> Result<String, JsValue> {
        let mut state = self.state.lock().await;
        state
            .core
            .add_runtime_client(&client_bundle)
            .map_err(js_error)?;
        Ok(state.core.untrusted_public_key_bundle())
    }

    #[wasm_bindgen(js_name = handle)]
    pub async fn handle(&self, frame: Vec<u8>) -> Result<Option<Vec<u8>>, JsValue> {
        let mut state = self.state.lock().await;
        state.core.handle_frame(&frame).await.map_err(js_error)
    }

    #[wasm_bindgen(js_name = configureWebDav)]
    pub async fn configure_webdav(
        &self,
        url: String,
        username: String,
        password: String,
    ) -> Result<(), JsValue> {
        let provider = WebDavProvider::new(url, Some(WebDavAuth::basic(username, password)))
            .map_err(js_error)?;
        self.state
            .lock()
            .await
            .core
            .register_storage_provider("webdav", Arc::new(provider));
        Ok(())
    }

    #[wasm_bindgen(js_name = configureLocalFile)]
    pub async fn configure_local_file(
        &self,
        file: File,
        handle: Option<FileSystemFileHandle>,
    ) -> Result<(), JsValue> {
        self.state
            .lock()
            .await
            .core
            .register_storage_provider("local-file", Arc::new(LocalFileStorage { file, handle }));
        Ok(())
    }
}
