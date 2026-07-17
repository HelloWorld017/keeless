use std::sync::Arc;
use std::time::Duration;

use keeless_kdbx::{
    open_database, save_database, CompositeKey, Database, DatabaseMerger, MergeResult,
    MergeStrategy,
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
            .field("path", &self.path)
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
        key: &CompositeKey,
        options: SyncOptions,
    ) -> Result<Self, SyncError> {
        let path = path.into();
        let remote = match provider.read(&path, None).await {
            Ok(remote) => remote,
            Err(error) if error.kind() == StorageErrorKind::NotFound => {
                return Err(SyncError::RemoteNotFound(path));
            }
            Err(error) => return Err(error.into()),
        };
        let database = open_database(remote.bytes.as_slice(), key)?;
        Ok(Self::from_remote(provider, path, database, remote, options))
    }

    /// Creates a new remote KDBX, or two-way merges with a concurrently existing file.
    pub async fn create(
        provider: Arc<dyn StorageProvider>,
        path: impl Into<String>,
        database: Database,
        key: &CompositeKey,
        options: SyncOptions,
    ) -> Result<Self, SyncError> {
        let path = path.into();
        let local_bytes = serialize_database(&database, key)?;
        let max_attempts = options.retry_policy.max_retries.saturating_add(1);
        for attempt in 0..max_attempts {
            match provider
                .write(&path, local_bytes.clone(), WriteCondition::MustNotExist)
                .await?
            {
                WriteOutcome::Applied { revision } => {
                    let persisted = open_database(local_bytes.as_slice(), key)?;
                    return Ok(Self {
                        provider,
                        path,
                        database: persisted,
                        checkpoint: Checkpoint {
                            bytes: local_bytes,
                            revision,
                        },
                        options,
                        dirty: false,
                    });
                }
                WriteOutcome::Conflict => {}
            }

            let remote = match provider.read(&path, None).await {
                Ok(remote) => remote,
                Err(error)
                    if error.kind() == StorageErrorKind::NotFound && attempt + 1 < max_attempts =>
                {
                    delay(&options.retry_policy, attempt).await;
                    continue;
                }
                Err(error) if error.kind() == StorageErrorKind::NotFound => {
                    return Err(SyncError::RetryExhausted {
                        attempts: max_attempts,
                    });
                }
                Err(error) => return Err(error.into()),
            };
            let revision = require_revision(&remote, &path)?;
            let mut target = database.clone();
            let source = open_database(remote.bytes.as_slice(), key)?;
            DatabaseMerger::new(options.merge_strategy).merge(&mut target, &source);
            // A two-way initial merge has no safe timestamp for database-level settings;
            // the newly created local database remains the metadata target.
            let merged_bytes = serialize_database(&target, key)?;

            match provider
                .write(
                    &path,
                    merged_bytes.clone(),
                    WriteCondition::MustMatch(revision),
                )
                .await?
            {
                WriteOutcome::Applied { revision } => {
                    let persisted = open_database(merged_bytes.as_slice(), key)?;
                    return Ok(Self {
                        provider,
                        path,
                        database: persisted,
                        checkpoint: Checkpoint {
                            bytes: merged_bytes,
                            revision,
                        },
                        options,
                        dirty: false,
                    });
                }
                WriteOutcome::Conflict if attempt + 1 < max_attempts => {
                    delay(&options.retry_policy, attempt).await;
                }
                WriteOutcome::Conflict => {
                    return Err(SyncError::RetryExhausted {
                        attempts: max_attempts,
                    });
                }
            }
        }

        unreachable!("create retry loop always returns")
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

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn checkpoint_revision(&self) -> Option<&Revision> {
        self.checkpoint.revision.as_ref()
    }

    /// Pulls remote changes, merges concurrent edits, and conditionally writes local changes.
    pub async fn sync(&mut self, key: &CompositeKey) -> Result<SyncReport, SyncError> {
        let max_attempts = self.options.retry_policy.max_retries.saturating_add(1);

        for attempt in 0..max_attempts {
            let remote = self.provider.read(&self.path, None).await?;

            if !self.dirty {
                if remote.bytes == self.checkpoint.bytes {
                    self.checkpoint.revision = remote.metadata.revision;
                    return Ok(SyncReport {
                        attempts: attempt + 1,
                        ..SyncReport::default()
                    });
                }

                let database = open_database(remote.bytes.as_slice(), key)?;
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

            let revision = require_revision(&remote, &self.path)?;
            let mut target = self.database.clone();
            let mut merge_result = MergeResult::default();
            let downloaded = remote.bytes != self.checkpoint.bytes;

            if downloaded {
                let source = open_database(remote.bytes.as_slice(), key)?;
                let base = open_database(self.checkpoint.bytes.as_slice(), key)?;
                merge_result = DatabaseMerger::new(self.options.merge_strategy).merge_three_way(
                    &mut target,
                    &source,
                    &base,
                );
            }

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
                    self.database = open_database(merged_bytes.as_slice(), key)?;
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

fn require_revision(remote: &RemoteFile, path: &str) -> Result<Revision, SyncError> {
    remote
        .metadata
        .revision
        .clone()
        .ok_or_else(|| SyncError::AtomicUpdateUnsupported(path.to_string()))
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
