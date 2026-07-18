use std::{collections::HashMap, rc::Rc, sync::Arc};

use futures::lock::Mutex;
use js_sys::{Array, Object};
use keeless_core::{ClientApprovalProvider, HostFuture, KeelessCore, KeelessHost, StorageProvider};
use keeless_sync::{WriteCondition, WriteOutcome};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};

use crate::{
    clock::BrowserClock,
    config::BrowserConfig,
    imported_database_path, numbered_database_path,
    storages::indexeddb::IndexedDbStorage,
    utils::indexeddb::{EntryKind, IndexedDb, js_error, set_property},
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
    storage: Rc<IndexedDbStorage>,
}

#[wasm_bindgen]
impl BrowserCore {
    #[wasm_bindgen(js_name = create)]
    // KeelessHost intentionally uses Arc for one API across native and single-threaded WASM.
    #[allow(clippy::arc_with_non_send_sync)]
    pub async fn create(default_approved_bundle: Option<String>) -> Result<BrowserCore, JsValue> {
        let idb = IndexedDb::open().await?;
        let storage = Rc::new(IndexedDbStorage { idb: idb.clone() });
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
            storage,
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

    #[wasm_bindgen(js_name = importDatabase)]
    pub async fn import_database(
        &self,
        file_name: String,
        bytes: Vec<u8>,
    ) -> Result<String, JsValue> {
        let base_path = imported_database_path(&file_name);
        let mut available_path = None;
        for number in 1..=10_000 {
            let path = numbered_database_path(&base_path, number);
            if self.storage.stat(&path).await.map_err(js_error)?.is_none() {
                available_path = Some(path);
                break;
            }
        }
        let path =
            available_path.ok_or_else(|| js_error("too many databases use the same file name"))?;
        match self
            .storage
            .write(&path, bytes, WriteCondition::MustNotExist)
            .await
            .map_err(js_error)?
        {
            WriteOutcome::Applied { .. } => Ok(path),
            WriteOutcome::Conflict => Err(js_error("database import conflicted; try again")),
        }
    }

    #[wasm_bindgen(js_name = listDatabases)]
    pub async fn list_databases(&self) -> Result<Array, JsValue> {
        let entries = self.storage.idb.entries().await.map_err(js_error)?;
        let databases = Array::new();
        for entry in entries {
            let Some(name) = entry.path.strip_prefix("databases/") else {
                continue;
            };
            if entry.kind != EntryKind::File || name.is_empty() || name.contains('/') {
                continue;
            }
            let object = Object::new();
            set_property(&object, "path", &JsValue::from_str(&entry.path)).map_err(js_error)?;
            set_property(&object, "name", &JsValue::from_str(name)).map_err(js_error)?;
            set_property(
                &object,
                "size",
                &JsValue::from_f64(entry.bytes.len() as f64),
            )
            .map_err(js_error)?;
            databases.push(&object);
        }
        Ok(databases)
    }
}
