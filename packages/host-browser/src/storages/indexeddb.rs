use std::rc::Rc;

use keeless_core::StorageProvider;
use keeless_sync::{
    ByteRange, FileMetadata, RemoteFile, Revision, StorageError, StorageErrorKind, StorageFuture,
    WriteCondition, WriteOutcome,
};
use rexie::TransactionMode;
use wasm_bindgen::JsValue;

use crate::{
    database_path, inclusive_range,
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
            let path = database_path(path)
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
            let path = database_path(path)
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
            let path = database_path(path)
                .map_err(|error| storage_error(StorageErrorKind::InvalidInput, error))?;
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
            let path = database_path(path)
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
}
