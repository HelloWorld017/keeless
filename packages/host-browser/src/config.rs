use std::rc::Rc;

use js_sys::{Object, Reflect, Uint8Array};
use keeless_core::{ConfigProvider, HostFuture};
use rexie::TransactionMode;
use wasm_bindgen::JsValue;

use crate::utils::indexeddb::{CONFIG_STORE, IndexedDb};

pub(crate) struct BrowserConfig {
    pub(crate) idb: Rc<IndexedDb>,
    pub(crate) key: &'static str,
}

impl ConfigProvider for BrowserConfig {
    fn load(&self) -> HostFuture<'_, keeless_core::Result<Option<Vec<u8>>>> {
        Box::pin(async move {
            let transaction = self
                .idb
                .db
                .transaction(&[CONFIG_STORE], TransactionMode::ReadOnly)
                .map_err(|error| keeless_core::CoreError::Host(error.to_string()))?;
            let value = transaction
                .store(CONFIG_STORE)
                .map_err(|error| keeless_core::CoreError::Host(error.to_string()))?
                .get(JsValue::from_str(self.key))
                .await
                .map_err(|error| keeless_core::CoreError::Host(error.to_string()))?;
            transaction
                .done()
                .await
                .map_err(|error| keeless_core::CoreError::Host(error.to_string()))?;
            value
                .map(|value| {
                    Reflect::get(&value, &JsValue::from_str("bytes"))
                        .map(|bytes| Uint8Array::new(&bytes).to_vec())
                        .map_err(|error| keeless_core::CoreError::Host(format!("{error:?}")))
                })
                .transpose()
        })
    }

    fn save<'a>(&'a self, config: &'a [u8]) -> HostFuture<'a, keeless_core::Result<()>> {
        Box::pin(async move {
            let object = Object::new();
            Reflect::set(
                &object,
                &JsValue::from_str("key"),
                &JsValue::from_str(self.key),
            )
            .map_err(|error| keeless_core::CoreError::Host(format!("{error:?}")))?;
            Reflect::set(
                &object,
                &JsValue::from_str("bytes"),
                &Uint8Array::from(config),
            )
            .map_err(|error| keeless_core::CoreError::Host(format!("{error:?}")))?;
            let transaction = self
                .idb
                .db
                .transaction(&[CONFIG_STORE], TransactionMode::ReadWrite)
                .map_err(|error| keeless_core::CoreError::Host(error.to_string()))?;
            transaction
                .store(CONFIG_STORE)
                .map_err(|error| keeless_core::CoreError::Host(error.to_string()))?
                .put(&object, None)
                .await
                .map_err(|error| keeless_core::CoreError::Host(error.to_string()))?;
            transaction
                .done()
                .await
                .map_err(|error| keeless_core::CoreError::Host(error.to_string()))?;
            Ok(())
        })
    }
}

impl keeless_lesswire::StateStore for BrowserConfig {
    fn load(&self) -> keeless_lesswire::WireFuture<'_, keeless_lesswire::Result<Option<Vec<u8>>>> {
        Box::pin(async move {
            let transaction = self
                .idb
                .db
                .transaction(&[CONFIG_STORE], TransactionMode::ReadOnly)
                .map_err(|error| keeless_lesswire::Error::Host(error.to_string()))?;
            let value = transaction
                .store(CONFIG_STORE)
                .map_err(|error| keeless_lesswire::Error::Host(error.to_string()))?
                .get(JsValue::from_str(self.key))
                .await
                .map_err(|error| keeless_lesswire::Error::Host(error.to_string()))?;
            transaction
                .done()
                .await
                .map_err(|error| keeless_lesswire::Error::Host(error.to_string()))?;
            value
                .map(|value| {
                    Reflect::get(&value, &JsValue::from_str("bytes"))
                        .map(|bytes| Uint8Array::new(&bytes).to_vec())
                        .map_err(|error| keeless_lesswire::Error::Host(format!("{error:?}")))
                })
                .transpose()
        })
    }

    fn save<'a>(
        &'a self,
        state: &'a [u8],
    ) -> keeless_lesswire::WireFuture<'a, keeless_lesswire::Result<()>> {
        Box::pin(async move {
            let object = Object::new();
            Reflect::set(
                &object,
                &JsValue::from_str("key"),
                &JsValue::from_str(self.key),
            )
            .map_err(|error| keeless_lesswire::Error::Host(format!("{error:?}")))?;
            Reflect::set(
                &object,
                &JsValue::from_str("bytes"),
                &Uint8Array::from(state),
            )
            .map_err(|error| keeless_lesswire::Error::Host(format!("{error:?}")))?;
            let transaction = self
                .idb
                .db
                .transaction(&[CONFIG_STORE], TransactionMode::ReadWrite)
                .map_err(|error| keeless_lesswire::Error::Host(error.to_string()))?;
            transaction
                .store(CONFIG_STORE)
                .map_err(|error| keeless_lesswire::Error::Host(error.to_string()))?
                .put(&object, None)
                .await
                .map_err(|error| keeless_lesswire::Error::Host(error.to_string()))?;
            transaction
                .done()
                .await
                .map_err(|error| keeless_lesswire::Error::Host(error.to_string()))?;
            Ok(())
        })
    }
}
