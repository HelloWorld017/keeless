use std::{collections::HashMap, rc::Rc, sync::Arc};

use futures::lock::Mutex;
use js_sys::{Array, Object, Reflect, Uint8Array};
use keeless_core::{
    ClientApprovalProvider, Clock, ConfigProvider, HostFuture, KeelessCore, KeelessHost,
    StorageProvider,
};
use keeless_sync::{
    ByteRange, FileMetadata, RemoteFile, Revision, StorageError, StorageErrorKind, StorageFuture,
    WriteCondition, WriteOutcome,
};
use rexie::{ObjectStore, Rexie, TransactionMode};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};

use crate::{imported_database_path, inclusive_range, normalize_path, numbered_database_path};

const DATABASE_NAME: &str = "keeless";
const CONFIG_STORE: &str = "config";
const ENTRY_STORE: &str = "entries";
const CONFIG_KEY: &str = "core";

fn js_error(error: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&error.to_string()).into()
}

fn storage_error(kind: StorageErrorKind, error: impl std::fmt::Display) -> StorageError {
    StorageError::new(kind, error.to_string())
}

fn idb_error(error: impl std::fmt::Display) -> StorageError {
    storage_error(StorageErrorKind::Other, format!("IndexedDB error: {error}"))
}

fn property(value: &JsValue, name: &str) -> Result<JsValue, StorageError> {
    Reflect::get(value, &JsValue::from_str(name)).map_err(|error| {
        storage_error(
            StorageErrorKind::Other,
            format!("IndexedDB record error: {error:?}"),
        )
    })
}

fn string_property(value: &JsValue, name: &str) -> Result<String, StorageError> {
    property(value, name)?
        .as_string()
        .ok_or_else(|| storage_error(StorageErrorKind::Other, format!("invalid {name} field")))
}

fn set_property(object: &Object, name: &str, value: &JsValue) -> Result<(), StorageError> {
    Reflect::set(object, &JsValue::from_str(name), value)
        .map(|_| ())
        .map_err(|error| {
            storage_error(
                StorageErrorKind::Other,
                format!("IndexedDB record error: {error:?}"),
            )
        })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    File,
    Directory,
}

struct Entry {
    path: String,
    kind: EntryKind,
    bytes: Vec<u8>,
    revision: String,
}

impl Entry {
    fn from_js(value: JsValue) -> Result<Self, StorageError> {
        let kind = match string_property(&value, "kind")?.as_str() {
            "file" => EntryKind::File,
            "directory" => EntryKind::Directory,
            _ => return Err(storage_error(StorageErrorKind::Other, "invalid entry kind")),
        };
        let bytes = if kind == EntryKind::File {
            Uint8Array::new(&property(&value, "bytes")?).to_vec()
        } else {
            Vec::new()
        };
        Ok(Self {
            path: string_property(&value, "path")?,
            kind,
            bytes,
            revision: string_property(&value, "revision")?,
        })
    }

    fn to_js(&self) -> Result<JsValue, StorageError> {
        let object = Object::new();
        set_property(&object, "path", &JsValue::from_str(&self.path))?;
        set_property(
            &object,
            "kind",
            &JsValue::from_str(if self.kind == EntryKind::File {
                "file"
            } else {
                "directory"
            }),
        )?;
        set_property(&object, "bytes", &Uint8Array::from(self.bytes.as_slice()))?;
        set_property(&object, "revision", &JsValue::from_str(&self.revision))?;
        Ok(object.into())
    }

    fn metadata(&self) -> FileMetadata {
        FileMetadata {
            size: self.bytes.len() as u64,
            revision: Some(Revision::StrongEtag(self.revision.clone())),
            last_modified: None,
        }
    }
}

struct IndexedDb {
    db: Rexie,
}

impl IndexedDb {
    async fn open() -> Result<Rc<Self>, JsValue> {
        let db = Rexie::builder(DATABASE_NAME)
            .version(1)
            .add_object_store(ObjectStore::new(CONFIG_STORE).key_path("key"))
            .add_object_store(ObjectStore::new(ENTRY_STORE).key_path("path"))
            .build()
            .await
            .map_err(js_error)?;
        Ok(Rc::new(Self { db }))
    }

    async fn get_entry(&self, path: &str) -> Result<Option<Entry>, StorageError> {
        let transaction = self
            .db
            .transaction(&[ENTRY_STORE], TransactionMode::ReadOnly)
            .map_err(idb_error)?;
        let value = transaction
            .store(ENTRY_STORE)
            .map_err(idb_error)?
            .get(JsValue::from_str(path))
            .await
            .map_err(idb_error)?;
        transaction.done().await.map_err(idb_error)?;
        value.map(Entry::from_js).transpose()
    }

    async fn entries(&self) -> Result<Vec<Entry>, StorageError> {
        let transaction = self
            .db
            .transaction(&[ENTRY_STORE], TransactionMode::ReadOnly)
            .map_err(idb_error)?;
        let values = transaction
            .store(ENTRY_STORE)
            .map_err(idb_error)?
            .get_all(None, None)
            .await
            .map_err(idb_error)?;
        transaction.done().await.map_err(idb_error)?;
        values.into_iter().map(Entry::from_js).collect()
    }
}

struct BrowserConfig {
    idb: Rc<IndexedDb>,
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
                .get(JsValue::from_str(CONFIG_KEY))
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
                &JsValue::from_str(CONFIG_KEY),
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

struct BrowserClock;

impl Clock for BrowserClock {
    fn now_millis(&self) -> i64 {
        js_sys::Date::now().min(i64::MAX as f64) as i64
    }

    fn monotonic_millis(&self) -> u64 {
        web_sys::window()
            .and_then(|window| window.performance())
            .map_or_else(js_sys::Date::now, |performance| performance.now())
            .max(0.0) as u64
    }
}

struct IndexedDbStorage {
    idb: Rc<IndexedDb>,
}

impl StorageProvider for IndexedDbStorage {
    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<ByteRange>,
    ) -> StorageFuture<'a, Result<RemoteFile, StorageError>> {
        Box::pin(async move {
            let path = normalize_path(path)
                .map_err(|error| storage_error(StorageErrorKind::InvalidInput, error))?;
            let entry =
                self.idb.get_entry(&path).await?.ok_or_else(|| {
                    storage_error(StorageErrorKind::NotFound, "file does not exist")
                })?;
            if entry.kind != EntryKind::File {
                return Err(storage_error(
                    StorageErrorKind::InvalidInput,
                    "path is a directory",
                ));
            }
            let metadata = entry.metadata();
            let bytes = match range {
                Some(range) => inclusive_range(&entry.bytes, range.start, range.end)
                    .map_err(|error| storage_error(StorageErrorKind::InvalidInput, error))?,
                None => entry.bytes,
            };
            Ok(RemoteFile { bytes, metadata })
        })
    }

    fn stat<'a>(
        &'a self,
        path: &'a str,
    ) -> StorageFuture<'a, Result<Option<FileMetadata>, StorageError>> {
        Box::pin(async move {
            let path = normalize_path(path)
                .map_err(|error| storage_error(StorageErrorKind::InvalidInput, error))?;
            Ok(self
                .idb
                .get_entry(&path)
                .await?
                .map(|entry| entry.metadata()))
        })
    }

    fn write<'a>(
        &'a self,
        path: &'a str,
        bytes: Vec<u8>,
        condition: WriteCondition,
    ) -> StorageFuture<'a, Result<WriteOutcome, StorageError>> {
        Box::pin(async move {
            let path = normalize_path(path)
                .map_err(|error| storage_error(StorageErrorKind::InvalidInput, error))?;
            if path.is_empty() {
                return Err(storage_error(
                    StorageErrorKind::InvalidInput,
                    "file path is empty",
                ));
            }
            let transaction = self
                .idb
                .db
                .transaction(&[ENTRY_STORE], TransactionMode::ReadWrite)
                .map_err(idb_error)?;
            let store = transaction.store(ENTRY_STORE).map_err(idb_error)?;
            let current = store
                .get(JsValue::from_str(&path))
                .await
                .map_err(idb_error)?
                .map(Entry::from_js)
                .transpose()?;
            if current
                .as_ref()
                .is_some_and(|entry| entry.kind == EntryKind::Directory)
            {
                return Err(storage_error(
                    StorageErrorKind::AlreadyExists,
                    "path is a directory",
                ));
            }
            let matches = match &condition {
                WriteCondition::Unconditional => true,
                WriteCondition::MustNotExist => current.is_none(),
                WriteCondition::MustMatch(Revision::StrongEtag(expected)) => current
                    .as_ref()
                    .is_some_and(|entry| &entry.revision == expected),
                WriteCondition::MustMatch(Revision::LastModified(_)) => false,
            };
            if !matches {
                return Ok(WriteOutcome::Conflict);
            }
            let revision = current
                .as_ref()
                .and_then(|entry| entry.revision.parse::<u64>().ok())
                .unwrap_or(0)
                .saturating_add(1)
                .to_string();
            let entry = Entry {
                path,
                kind: EntryKind::File,
                bytes,
                revision: revision.clone(),
            };
            store.put(&entry.to_js()?, None).await.map_err(idb_error)?;
            transaction.done().await.map_err(idb_error)?;
            Ok(WriteOutcome::Applied {
                revision: Some(Revision::StrongEtag(revision)),
            })
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> StorageFuture<'a, Result<(), StorageError>> {
        Box::pin(async move {
            let path = normalize_path(path)
                .map_err(|error| storage_error(StorageErrorKind::InvalidInput, error))?;
            let transaction = self
                .idb
                .db
                .transaction(&[ENTRY_STORE], TransactionMode::ReadWrite)
                .map_err(idb_error)?;
            transaction
                .store(ENTRY_STORE)
                .map_err(idb_error)?
                .delete(JsValue::from_str(&path))
                .await
                .map_err(idb_error)?;
            transaction.done().await.map_err(idb_error)?;
            Ok(())
        })
    }

    fn ensure_directory<'a>(
        &'a self,
        path: &'a str,
    ) -> StorageFuture<'a, Result<(), StorageError>> {
        Box::pin(async move {
            let path = normalize_path(path)
                .map_err(|error| storage_error(StorageErrorKind::InvalidInput, error))?;
            let mut current = String::new();
            for segment in path.split('/').filter(|segment| !segment.is_empty()) {
                if !current.is_empty() {
                    current.push('/');
                }
                current.push_str(segment);
                let transaction = self
                    .idb
                    .db
                    .transaction(&[ENTRY_STORE], TransactionMode::ReadWrite)
                    .map_err(idb_error)?;
                let store = transaction.store(ENTRY_STORE).map_err(idb_error)?;
                let existing = store
                    .get(JsValue::from_str(&current))
                    .await
                    .map_err(idb_error)?
                    .map(Entry::from_js)
                    .transpose()?;
                match existing {
                    Some(entry) if entry.kind != EntryKind::Directory => {
                        return Err(storage_error(
                            StorageErrorKind::AlreadyExists,
                            "path is a file",
                        ));
                    }
                    Some(_) => {}
                    None => {
                        let entry = Entry {
                            path: current.clone(),
                            kind: EntryKind::Directory,
                            bytes: Vec::new(),
                            revision: "1".into(),
                        };
                        store.add(&entry.to_js()?, None).await.map_err(idb_error)?;
                    }
                }
                transaction.done().await.map_err(idb_error)?;
            }
            Ok(())
        })
    }

    fn list<'a>(&'a self, path: &'a str) -> StorageFuture<'a, Result<Vec<String>, StorageError>> {
        Box::pin(async move { self.list_kind(path, EntryKind::File).await })
    }

    fn read_directory<'a>(
        &'a self,
        path: &'a str,
    ) -> StorageFuture<'a, Result<Vec<String>, StorageError>> {
        Box::pin(async move { self.list_kind(path, EntryKind::Directory).await })
    }
}

impl IndexedDbStorage {
    async fn list_kind(
        &self,
        path: &str,
        expected_kind: EntryKind,
    ) -> Result<Vec<String>, StorageError> {
        let path = normalize_path(path)
            .map_err(|error| storage_error(StorageErrorKind::InvalidInput, error))?;
        if !path.is_empty() {
            match self.idb.get_entry(&path).await? {
                Some(entry) if entry.kind == EntryKind::Directory => {}
                Some(_) => {
                    return Err(storage_error(
                        StorageErrorKind::InvalidInput,
                        "path is a file",
                    ));
                }
                None => {
                    return Err(storage_error(
                        StorageErrorKind::NotFound,
                        "directory does not exist",
                    ));
                }
            }
        }
        let prefix = if path.is_empty() {
            String::new()
        } else {
            format!("{path}/")
        };
        let mut names = self
            .idb
            .entries()
            .await?
            .into_iter()
            .filter(|entry| entry.kind == expected_kind)
            .filter_map(|entry| {
                let relative = entry.path.strip_prefix(&prefix)?;
                (!relative.is_empty() && !relative.contains('/')).then(|| relative.to_owned())
            })
            .collect::<Vec<_>>();
        names.sort();
        Ok(names)
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

    #[wasm_bindgen(js_name = processFrame)]
    pub async fn process_frame(&self, frame: Vec<u8>) -> Result<Option<Vec<u8>>, JsValue> {
        self.core
            .lock()
            .await
            .process_frame_json(&frame)
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
        self.storage
            .ensure_directory("databases")
            .await
            .map_err(js_error)?;
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
