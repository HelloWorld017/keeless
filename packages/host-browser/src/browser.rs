use std::{collections::HashMap, rc::Rc, sync::Arc};

use futures::lock::Mutex;
use keeless_core::{ClientApprovalProvider, HostFuture, KeelessCore, KeelessHost, StorageProvider};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};

use crate::{
    clock::BrowserClock,
    config::BrowserConfig,
    storages::indexeddb::IndexedDbStorage,
    utils::indexeddb::{IndexedDb, js_error},
};

struct BrowserApproval;

impl ClientApprovalProvider for BrowserApproval {
    fn approve(&self, public_key_bundle: &str) -> HostFuture<'_, keeless_core::Result<bool>> {
        let message = format!("Allow this client to access Keeless?\n\n{public_key_bundle}");
        Box::pin(async move {
            web_sys::window()
                .ok_or_else(|| keeless_core::CoreError::Host("window is unavailable".into()))?
                .confirm_with_message(&message)
                .map_err(|error| keeless_core::CoreError::Host(format!("{error:?}")))
        })
    }
}

#[wasm_bindgen]
pub struct BrowserCore {
    core: Rc<Mutex<KeelessCore>>,
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
            "idb".into(),
            Arc::new(IndexedDbStorage { idb: idb.clone() }),
        );
        let host = KeelessHost {
            default_approved_keys: default_approved_bundle.into_iter().collect(),
            storage_providers,
            config_provider: Arc::new(BrowserConfig { idb }),
            approval_provider: Arc::new(BrowserApproval),
            clock: Arc::new(BrowserClock),
        };
        let core = KeelessCore::new(host).await.map_err(js_error)?;
        Ok(Self {
            core: Rc::new(Mutex::new(core)),
        })
    }

    #[wasm_bindgen(js_name = handle)]
    pub async fn handle(&self, frame: Vec<u8>) -> Result<Option<Vec<u8>>, JsValue> {
        self.core
            .lock()
            .await
            .handle(&frame)
            .await
            .map_err(js_error)
    }
}
