use std::sync::Arc;
use std::time::Duration;

use keeless_kdbx::{
    open_database, open_database_with_key, save_database, CompositeCredentials, CompositeKey,
    Database, DatabaseMerger, MergeResult, MergeStrategy,
};

use crate::{
    RemoteFile, Revision, StorageErrorKind, StorageProvider, SyncError, WriteCondition,
    WriteOutcome,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Number of CAS conflicts allowed after the first write attempt.
    pub max_retries: usize,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(2),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncOptions {
    pub merge_strategy: MergeStrategy,
    pub retry_policy: RetryPolicy,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            merge_strategy: MergeStrategy::NewestWins,
            retry_policy: RetryPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    pub attempts: usize,
    pub uploaded: bool,
    pub downloaded: bool,
    pub merge_result: MergeResult,
}

#[derive(Debug)]
struct Checkpoint {
    bytes: Vec<u8>,
    revision: Option<Revision>,
}

/// An opened KDBX file and the last remote representation synchronized with it.
pub struct FileHandle {
    provider: Arc<dyn StorageProvider>,
    path: String,
    database: Database,
    checkpoint: Checkpoint,
    options: SyncOptions,
    dirty: bool,
}

impl std::fmt::Debug for FileHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileHandle")
            .field("entry_count", &self.database.entry_count())
            .field("group_count", &self.database.group_count())
            .field("checkpoint_revision", &self.checkpoint.revision)
            .field("options", &self.options)
            .field("dirty", &self.dirty)
            .finish_non_exhaustive()
    }
}

impl FileHandle {
    /// Opens an existing remote KDBX and records that exact representation as the base checkpoint.
    pub async fn open(
        provider: Arc<dyn StorageProvider>,
        path: impl Into<String>,
        credentials: &CompositeCredentials,
        options: SyncOptions,
    ) -> Result<(Self, CompositeKey), SyncError> {
        let path = path.into();
        let remote = match provider.read(&path, None).await {
            Ok(remote) => remote,
            Err(error) if error.kind() == StorageErrorKind::NotFound => {
                return Err(SyncError::RemoteNotFound);
            }
            Err(error) => return Err(error.into()),
        };
        let opened = open_database(remote.bytes.as_slice(), credentials)?;
        let key = opened.key;
        Ok((
            Self::from_remote(provider, path, opened.database, remote, options),
            key,
        ))
    }

    /// Opens an existing remote file using a transformed key without a KDF.
    pub async fn open_with_key(
        provider: Arc<dyn StorageProvider>,
        path: impl Into<String>,
        key: &CompositeKey,
        options: SyncOptions,
    ) -> Result<Self, SyncError> {
        let path = path.into();
        let remote = match provider.read(&path, None).await {
            Ok(remote) => remote,
            Err(error) if error.kind() == StorageErrorKind::NotFound => {
                return Err(SyncError::RemoteNotFound);
            }
            Err(error) => return Err(error.into()),
        };
        let database = open_with_key(remote.bytes.as_slice(), key)?;
        Ok(Self::from_remote(provider, path, database, remote, options))
    }

    /// Opens a locally cached remote representation without contacting storage.
    pub fn open_cached_with_credentials(
        provider: Arc<dyn StorageProvider>,
        path: impl Into<String>,
        bytes: Vec<u8>,
        credentials: &CompositeCredentials,
        options: SyncOptions,
    ) -> Result<(Self, CompositeKey), SyncError> {
        let path = path.into();
        let opened = open_database(bytes.as_slice(), credentials)?;
        let key = opened.key;
        Ok((
            Self {
                provider,
                path,
                database: opened.database,
                checkpoint: Checkpoint {
                    bytes,
                    revision: None,
                },
                options,
                dirty: false,
            },
            key,
        ))
    }

    /// Opens a cached representation with an already-derived key.
    pub fn open_cached(
        provider: Arc<dyn StorageProvider>,
        path: impl Into<String>,
        bytes: Vec<u8>,
        key: &CompositeKey,
        options: SyncOptions,
    ) -> Result<Self, SyncError> {
        let path = path.into();
        let database = open_with_key(bytes.as_slice(), key)?;
        Ok(Self {
            provider,
            path,
            database,
            checkpoint: Checkpoint {
                bytes,
                revision: None,
            },
            options,
            dirty: false,
        })
    }

    /// Creates a new remote KDBX without merging with an existing file.
    pub async fn create(
        provider: Arc<dyn StorageProvider>,
        path: impl Into<String>,
        database: Database,
        key: &CompositeKey,
        options: SyncOptions,
    ) -> Result<Self, SyncError> {
        let path = path.into();
        let bytes = serialize_database(&database, key)?;
        let revision = match provider
            .write(&path, bytes.clone(), WriteCondition::MustNotExist)
            .await?
        {
            WriteOutcome::Applied { revision } => revision,
            WriteOutcome::Conflict => {
                return Err(crate::StorageError::new(
                    StorageErrorKind::AlreadyExists,
                    "remote file already exists",
                )
                .into());
            }
        };
        let database = open_with_key(bytes.as_slice(), key)?;
        Ok(Self {
            provider,
            path,
            database,
            checkpoint: Checkpoint { bytes, revision },
            options,
            dirty: false,
        })
    }

    fn from_remote(
        provider: Arc<dyn StorageProvider>,
        path: String,
        database: Database,
        remote: RemoteFile,
        options: SyncOptions,
    ) -> Self {
        Self {
            provider,
            path,
            database,
            checkpoint: Checkpoint {
                bytes: remote.bytes,
                revision: remote.metadata.revision,
            },
            options,
            dirty: false,
        }
    }

    pub fn database(&self) -> &Database {
        &self.database
    }

    /// Returns the mutable database and conservatively marks the handle dirty.
    pub fn database_mut(&mut self) -> &mut Database {
        self.dirty = true;
        &mut self.database
    }

    /// Applies a mutation that provides its own atomicity and marks the handle dirty on change.
    ///
    /// The mutation must leave the database unchanged when it returns an error or `false`.
    pub fn apply_update<E>(
        &mut self,
        mutate: impl FnOnce(&mut Database) -> Result<bool, E>,
    ) -> Result<bool, E> {
        let changed = mutate(&mut self.database)?;
        if changed {
            self.dirty = true;
        }
        Ok(changed)
    }

    /// Commits a mutation which was fully validated and durably journaled by the caller.
    pub fn commit_prepared(&mut self, commit: impl FnOnce(&mut Database)) {
        commit(&mut self.database);
        self.dirty = true;
    }

    /// Mutable access reserved for applying an already authenticated local journal.
    pub fn replay_database(&mut self) -> &mut Database {
        &mut self.database
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn checkpoint_revision(&self) -> Option<&Revision> {
        self.checkpoint.revision.as_ref()
    }

    pub fn checkpoint_bytes(&self) -> &[u8] {
        &self.checkpoint.bytes
    }

    /// Derive a transformed key against this handle's exact base checkpoint.
    pub fn derive_key(
        &self,
        credentials: &CompositeCredentials,
    ) -> Result<CompositeKey, SyncError> {
        Ok(open_database(self.checkpoint.bytes.as_slice(), credentials)?.key)
    }

    /// Pulls remote changes, merges concurrent edits, and conditionally writes local changes.
    pub async fn sync(&mut self, key: &CompositeKey) -> Result<SyncReport, SyncError> {
        let remote = self.provider.read(&self.path, None).await?;
        self.sync_from_remote(key, remote).await
    }

    /// Synchronizes using a remote representation fetched by a host background task.
    pub async fn sync_from_remote(
        &mut self,
        key: &CompositeKey,
        remote: RemoteFile,
    ) -> Result<SyncReport, SyncError> {
        let max_attempts = self.options.retry_policy.max_retries.saturating_add(1);
        let mut first_remote = Some(remote);

        for attempt in 0..max_attempts {
            let remote = match first_remote.take() {
                Some(remote) => remote,
                None => self.provider.read(&self.path, None).await?,
            };

            if !self.dirty {
                if remote.bytes == self.checkpoint.bytes {
                    self.checkpoint.revision = remote.metadata.revision;
                    return Ok(SyncReport {
                        attempts: attempt + 1,
                        ..SyncReport::default()
                    });
                }

                let database = open_with_key(remote.bytes.as_slice(), key)?;
                if database.root_group_id != self.database.root_group_id {
                    return Err(SyncError::RootGroupMismatch);
                }
                self.database = database;
                self.checkpoint = Checkpoint {
                    bytes: remote.bytes,
                    revision: remote.metadata.revision,
                };
                return Ok(SyncReport {
                    attempts: attempt + 1,
                    downloaded: true,
                    ..SyncReport::default()
                });
            }

            // Stage remote merging so failed serialization or writes cannot alter dirty local data.
            let mut target = self.database.clone();
            let mut merge_result = MergeResult::default();
            let downloaded = remote.bytes != self.checkpoint.bytes;

            if downloaded {
                let source = open_with_key(remote.bytes.as_slice(), key)?;
                if source.root_group_id != self.database.root_group_id {
                    return Err(SyncError::RootGroupMismatch);
                }
                let base = open_with_key(self.checkpoint.bytes.as_slice(), key)?;
                merge_result = DatabaseMerger::with_credentials(self.options.merge_strategy, key)
                    .merge_three_way(&mut target, &source, &base);
            }

            let revision = require_revision(&remote)?;
            let merged_bytes = serialize_database(&target, key)?;
            match self
                .provider
                .write(
                    &self.path,
                    merged_bytes.clone(),
                    WriteCondition::MustMatch(revision),
                )
                .await?
            {
                WriteOutcome::Applied { revision } => {
                    self.database = open_with_key(merged_bytes.as_slice(), key)?;
                    self.checkpoint = Checkpoint {
                        bytes: merged_bytes,
                        revision,
                    };
                    self.dirty = false;
                    return Ok(SyncReport {
                        attempts: attempt + 1,
                        uploaded: true,
                        downloaded,
                        merge_result,
                    });
                }
                WriteOutcome::Conflict if attempt + 1 < max_attempts => {
                    delay(&self.options.retry_policy, attempt).await;
                }
                WriteOutcome::Conflict => {
                    return Err(SyncError::RetryExhausted {
                        attempts: max_attempts,
                    });
                }
            }
        }

        unreachable!("sync retry loop always returns")
    }
}

fn serialize_database(database: &Database, key: &CompositeKey) -> Result<Vec<u8>, SyncError> {
    let mut bytes = Vec::new();
    save_database(&mut bytes, database, key)?;
    Ok(bytes)
}

fn open_with_key(bytes: &[u8], key: &CompositeKey) -> Result<Database, SyncError> {
    match open_database_with_key(bytes, key) {
        Err(keeless_kdbx::DatabaseError::KdfParametersMismatch) => {
            Err(SyncError::CredentialsRequired)
        }
        Err(error) => Err(error.into()),
        Ok(database) => Ok(database),
    }
}

fn require_revision(remote: &RemoteFile) -> Result<Revision, SyncError> {
    remote
        .metadata
        .revision
        .clone()
        .ok_or(SyncError::AtomicUpdateUnsupported)
}

async fn delay(policy: &RetryPolicy, attempt: usize) {
    let factor = 1u32.checked_shl(attempt.min(31) as u32).unwrap_or(u32::MAX);
    let duration = policy
        .base_delay
        .saturating_mul(factor)
        .min(policy.max_delay);
    if duration.is_zero() {
        return;
    }

    #[cfg(not(target_arch = "wasm32"))]
    futures_timer::Delay::new(duration).await;

    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(duration.as_millis().min(u32::MAX as u128) as u32)
        .await;
}
