use std::rc::Rc;

use keeless_core::StorageProvider;
use keeless_sync::{
    ByteRange, FileMetadata, RemoteFile, Revision, StorageError, StorageErrorKind, StorageFuture,
    WriteCondition, WriteOutcome,
};
use rexie::TransactionMode;
use wasm_bindgen::JsValue;

use crate::{
    inclusive_range, normalize_path,
    utils::indexeddb::{ENTRY_STORE, Entry, EntryKind, IndexedDb, idb_error, storage_error},
};

pub(crate) struct IndexedDbStorage {
    pub(crate) idb: Rc<IndexedDb>,
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
