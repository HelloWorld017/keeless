use std::{collections::HashMap, rc::Rc, sync::Arc};

use futures::lock::Mutex;
use gloo_timers::future::TimeoutFuture;
use keeless_core::{
    CoreError, HostFuture, KeelessCore, KeelessHost, StorageProvider, TaskSpawner, TransferProvider,
};
use keeless_lesswire::{
    ApprovalProvider, MessageFrame, Server, ServerHost, TransferId, TransferOwner,
    TransferRegistry, WireFuture,
};
use keeless_sync::{WebDavAuth, WebDavProvider};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
use wasm_bindgen_futures::spawn_local;
use web_sys::{File, FileSystemFileHandle};
use zeroize::Zeroizing;

use crate::{
    clock::BrowserClock,
    config::BrowserConfig,
    storages::{indexeddb::IndexedDbStorage, local_file::LocalFileStorage},
    utils::indexeddb::{CORE_CONFIG_KEY, IndexedDb, WIRE_CONFIG_KEY, js_error},
};

struct BrowserApproval;

struct BrowserTaskSpawner;

#[derive(Clone)]
struct BrowserTransfers(TransferRegistry);

impl TransferProvider for BrowserTransfers {
    fn publish_download(
        &self,
        owner: &str,
        bytes: Zeroizing<Vec<u8>>,
    ) -> keeless_core::Result<String> {
        self.0
            .publish_download(TransferOwner::new(owner), bytes)
            .map(|id| id.encode())
            .map_err(|error| CoreError::Host(error.to_string()))
    }

    fn consume_upload(
        &self,
        owner: &str,
        transfer_id: &str,
    ) -> keeless_core::Result<Zeroizing<Vec<u8>>> {
        let id = TransferId::parse(transfer_id)
            .ok_or_else(|| CoreError::Host("invalid binary transfer ID".into()))?;
        self.0
            .consume_upload(&TransferOwner::new(owner), &id)
            .map_err(|error| CoreError::Host(error.to_string()))
    }

    fn clear(&self) {
        self.0.clear();
    }
}

impl TaskSpawner for BrowserTaskSpawner {
    fn spawn(&self, task: HostFuture<'static, ()>) {
        spawn_local(task);
    }
}

impl ApprovalProvider for BrowserApproval {
    fn approve(&self, _: &str) -> WireFuture<'_, keeless_lesswire::Result<bool>> {
        Box::pin(async { Ok(false) })
    }
}

struct BrowserState {
    core: KeelessCore,
    server: Server,
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
    pub async fn create(default_approved_bundle: Option<String>) -> Result<BrowserCore, JsValue> {
        let idb = IndexedDb::open().await?;
        let mut storage_providers: HashMap<String, Arc<dyn StorageProvider>> = HashMap::new();
        storage_providers.insert(
            "indexeddb".into(),
            Arc::new(IndexedDbStorage { idb: idb.clone() }),
        );
        let server = Server::new(ServerHost {
            store: Arc::new(BrowserConfig {
                idb: idb.clone(),
                key: WIRE_CONFIG_KEY,
            }),
            approval_provider: Arc::new(BrowserApproval),
            clock: Arc::new(BrowserClock),
            runtime_approved_clients: default_approved_bundle.into_iter().collect(),
        })
        .await
        .map_err(js_error)?;
        let host = KeelessHost {
            storage_providers,
            config_provider: Arc::new(BrowserConfig {
                idb: idb.clone(),
                key: CORE_CONFIG_KEY,
            }),
            password_input: None,
            passkey_consent: None,
            clock: Arc::new(BrowserClock),
            database_persistence: None,
            task_spawner: Some(Arc::new(BrowserTaskSpawner)),
            transfer_provider: Some(Arc::new(BrowserTransfers(server.transfers()))),
        };
        let core = KeelessCore::new(host).await.map_err(js_error)?;
        let state = Rc::new(Mutex::new(BrowserState { core, server }));
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

    #[wasm_bindgen(js_name = handle)]
    pub async fn handle(&self, frame: Vec<u8>) -> Result<Option<Vec<u8>>, JsValue> {
        if frame.len() > keeless_lesswire::MAX_FRAME_SIZE {
            return Ok(None);
        }
        let Ok(frame) = serde_json::from_slice::<MessageFrame>(&frame) else {
            return Ok(None);
        };
        let mut state = self.state.lock().await;
        let BrowserState { core, server } = &mut *state;
        server
            .handle_frame(&frame, |owner, plaintext| async move {
                core.handle_payload_from(Some(owner.as_str().into()), &plaintext)
                    .await
            })
            .await
            .map_err(js_error)?
            .map(|response| serde_json::to_vec(&response).map_err(js_error))
            .transpose()
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
