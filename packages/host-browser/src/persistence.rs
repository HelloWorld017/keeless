use std::sync::Mutex;

use js_sys::{Object, Reflect, Uint8Array};
use keeless_core::{CoreError, DatabaseId, DatabasePersistence, HostFuture, Result};
use rexie::TransactionMode;

use crate::utils::indexeddb::{IndexedDb, STATE_STORE};

const MAX_STATE_RECORD_SIZE: usize = 128 * 1024;

pub(crate) struct BrowserDatabasePersistence {
    idb: std::rc::Rc<IndexedDb>,
    selected: Mutex<Option<DatabaseId>>,
}

impl BrowserDatabasePersistence {
    pub(crate) fn new(idb: std::rc::Rc<IndexedDb>) -> Self {
        Self {
            idb,
            selected: Mutex::new(None),
        }
    }

    fn state_key(&self, name: &str) -> Result<String> {
        let selected = self
            .selected
            .lock()
            .map_err(|_| CoreError::Host("browser state lock was poisoned".into()))?;
        let database_id = selected
            .as_ref()
            .ok_or_else(|| CoreError::Host("database persistence is not selected".into()))?;
        match name {
            "config" | "core-wire-state" => Ok(format!("{}:{name}", database_id.encoded())),
            _ => Err(CoreError::Host("invalid database state record name".into())),
        }
    }

    fn state_keys(database_id: &DatabaseId) -> [String; 2] {
        let id = database_id.encoded();
        [format!("{id}:config"), format!("{id}:core-wire-state")]
    }
}

impl DatabasePersistence for BrowserDatabasePersistence {
    fn select<'a>(&'a self, database_id: &'a DatabaseId) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            *self
                .selected
                .lock()
                .map_err(|_| CoreError::Host("browser state lock was poisoned".into()))? =
                Some(database_id.clone());
            Ok(())
        })
    }

    fn purge<'a>(&'a self, database_id: &'a DatabaseId) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            let transaction = self
                .idb
                .db
                .transaction(&[STATE_STORE], TransactionMode::ReadWrite)
                .map_err(|error| CoreError::Host(error.to_string()))?;
            let store = transaction
                .store(STATE_STORE)
                .map_err(|error| CoreError::Host(error.to_string()))?;
            for key in Self::state_keys(database_id) {
                store
                    .delete(key.into())
                    .await
                    .map_err(|error| CoreError::Host(error.to_string()))?;
            }
            transaction
                .done()
                .await
                .map_err(|error| CoreError::Host(error.to_string()))?;
            Ok(())
        })
    }

    fn read_cache(&self) -> HostFuture<'_, Result<Option<Vec<u8>>>> {
        Box::pin(async { Ok(None) })
    }
    fn write_cache<'a>(&'a self, _: &'a [u8]) -> HostFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn read_journal(&self) -> HostFuture<'_, Result<Vec<Vec<u8>>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn append_journal<'a>(&'a self, _: &'a [u8]) -> HostFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn clear_journal(&self) -> HostFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn quarantine_cache<'a>(&'a self, _: &'a str) -> HostFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn quarantine_journal<'a>(&'a self, _: &'a str) -> HostFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn read_state_record<'a>(&'a self, name: &'a str) -> HostFuture<'a, Result<Option<Vec<u8>>>> {
        Box::pin(async move {
            let key = self.state_key(name)?;
            let transaction = self
                .idb
                .db
                .transaction(&[STATE_STORE], TransactionMode::ReadOnly)
                .map_err(|error| CoreError::Host(error.to_string()))?;
            let value = transaction
                .store(STATE_STORE)
                .map_err(|error| CoreError::Host(error.to_string()))?
                .get(key.into())
                .await
                .map_err(|error| CoreError::Host(error.to_string()))?;
            transaction
                .done()
                .await
                .map_err(|error| CoreError::Host(error.to_string()))?;
            value
                .map(|value| {
                    let bytes = Reflect::get(&value, &"bytes".into())
                        .map_err(|error| CoreError::Host(format!("{error:?}")))?;
                    let bytes = Uint8Array::new(&bytes).to_vec();
                    if bytes.len() > MAX_STATE_RECORD_SIZE {
                        return Err(CoreError::InvalidConfig(
                            "encrypted state record is too large".into(),
                        ));
                    }
                    Ok(bytes)
                })
                .transpose()
        })
    }

    fn write_state_record<'a>(
        &'a self,
        name: &'a str,
        bytes: &'a [u8],
    ) -> HostFuture<'a, Result<()>> {
        Box::pin(async move {
            if bytes.len() > MAX_STATE_RECORD_SIZE {
                return Err(CoreError::InvalidConfig(
                    "encrypted state record is too large".into(),
                ));
            }
            let key = self.state_key(name)?;
            let object = Object::new();
            Reflect::set(&object, &"key".into(), &key.into())
                .map_err(|error| CoreError::Host(format!("{error:?}")))?;
            Reflect::set(&object, &"bytes".into(), &Uint8Array::from(bytes).into())
                .map_err(|error| CoreError::Host(format!("{error:?}")))?;
            let transaction = self
                .idb
                .db
                .transaction(&[STATE_STORE], TransactionMode::ReadWrite)
                .map_err(|error| CoreError::Host(error.to_string()))?;
            transaction
                .store(STATE_STORE)
                .map_err(|error| CoreError::Host(error.to_string()))?
                .put(&object, None)
                .await
                .map_err(|error| CoreError::Host(error.to_string()))?;
            transaction
                .done()
                .await
                .map_err(|error| CoreError::Host(error.to_string()))?;
            Ok(())
        })
    }
}
