use std::rc::Rc;

use js_sys::{Object, Reflect, Uint8Array};
use keeless_sync::{FileMetadata, Revision, StorageError, StorageErrorKind};
use rexie::{ObjectStore, Rexie, TransactionMode};
use wasm_bindgen::JsValue;

const DATABASE_NAME: &str = "keeless";
pub(crate) const CONFIG_STORE: &str = "config";
pub(crate) const ENTRY_STORE: &str = "entries";
pub(crate) const CONFIG_KEY: &str = "core";

pub(crate) fn js_error(error: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&error.to_string()).into()
}

pub(crate) fn storage_error(kind: StorageErrorKind, error: impl std::fmt::Display) -> StorageError {
    StorageError::new(kind, error.to_string())
}

pub(crate) fn idb_error(error: impl std::fmt::Display) -> StorageError {
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

pub(crate) fn set_property(
    object: &Object,
    name: &str,
    value: &JsValue,
) -> Result<(), StorageError> {
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
pub(crate) enum EntryKind {
    File,
    Directory,
}

pub(crate) struct Entry {
    pub(crate) path: String,
    pub(crate) kind: EntryKind,
    pub(crate) bytes: Vec<u8>,
    pub(crate) revision: String,
}

impl Entry {
    pub(crate) fn from_js(value: JsValue) -> Result<Self, StorageError> {
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

    pub(crate) fn to_js(&self) -> Result<JsValue, StorageError> {
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

    pub(crate) fn metadata(&self) -> FileMetadata {
        FileMetadata {
            size: self.bytes.len() as u64,
            revision: Some(Revision::StrongEtag(self.revision.clone())),
            last_modified: None,
        }
    }
}

pub(crate) struct IndexedDb {
    pub(crate) db: Rexie,
}

impl IndexedDb {
    pub(crate) async fn open() -> Result<Rc<Self>, JsValue> {
        let db = Rexie::builder(DATABASE_NAME)
            .version(1)
            .add_object_store(ObjectStore::new(CONFIG_STORE).key_path("key"))
            .add_object_store(ObjectStore::new(ENTRY_STORE).key_path("path"))
            .build()
            .await
            .map_err(js_error)?;
        Ok(Rc::new(Self { db }))
    }

    pub(crate) async fn get_entry(&self, path: &str) -> Result<Option<Entry>, StorageError> {
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

    pub(crate) async fn entries(&self) -> Result<Vec<Entry>, StorageError> {
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
