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
                let local_icons = target.custom_icons.clone();
                merge_result = DatabaseMerger::new(self.options.merge_strategy).merge_three_way(
                    &mut target,
                    &source,
                    &base,
                );
                // The generic merger unions icons without consulting the base. Restore
                // the local side before applying deletion-aware three-way semantics.
                target.custom_icons = local_icons;
                merge_database_metadata(&mut target, &source, &base);
                merge_custom_icons_three_way(&mut target, &source, &base);
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

/// Applies a remote metadata value only when the local value still equals the base.
/// If both sides changed, local wins because these fields have no reliable timestamps.
fn merge_database_metadata(target: &mut Database, source: &Database, base: &Database) {
    merge_copy(&mut target.version, source.version, base.version);
    merge_copy(
        &mut target.file_version,
        source.file_version,
        base.file_version,
    );
    merge_copy(
        &mut target.encryption_algorithm,
        source.encryption_algorithm,
        base.encryption_algorithm,
    );
    merge_copy(
        &mut target.compression,
        source.compression,
        base.compression,
    );
    merge_clone(
        &mut target.kdf_parameters,
        &source.kdf_parameters,
        &base.kdf_parameters,
    );
    merge_clone(
        &mut target.public_custom_data,
        &source.public_custom_data,
        &base.public_custom_data,
    );
    merge_clone(
        &mut target.header_comment,
        &source.header_comment,
        &base.header_comment,
    );
    merge_clone(&mut target.name, &source.name, &base.name);
    merge_clone(
        &mut target.description,
        &source.description,
        &base.description,
    );
    merge_clone(
        &mut target.default_username,
        &source.default_username,
        &base.default_username,
    );
    merge_copy(
        &mut target.recycle_bin_uuid,
        source.recycle_bin_uuid,
        base.recycle_bin_uuid,
    );
    merge_copy(
        &mut target.entry_templates_uuid,
        source.entry_templates_uuid,
        base.entry_templates_uuid,
    );
    merge_clone(
        &mut target.memory_protection,
        &source.memory_protection,
        &base.memory_protection,
    );
    merge_custom_data_three_way(target, source, base);
    merge_clone(
        &mut target.xml_extensions,
        &source.xml_extensions,
        &base.xml_extensions,
    );
    if target.contains_unsupported_xml == base.contains_unsupported_xml
        && source.contains_unsupported_xml != base.contains_unsupported_xml
    {
        target.contains_unsupported_xml = source.contains_unsupported_xml;
    }
}

fn merge_custom_data_three_way(target: &mut Database, source: &Database, base: &Database) {
    if target.custom_data == base.custom_data {
        if source.custom_data != base.custom_data {
            target.custom_data = source.custom_data.clone();
        }
        return;
    }
    if source.custom_data == base.custom_data {
        return;
    }

    let keys: std::collections::HashSet<_> = base
        .custom_data
        .iter()
        .chain(source.custom_data.iter())
        .chain(target.custom_data.iter())
        .map(|(key, _)| key.clone())
        .collect();
    for key in keys {
        let base_value = base
            .custom_data
            .iter()
            .find(|(name, _)| *name == &key)
            .map(|(_, item)| item);
        let source_value = source
            .custom_data
            .iter()
            .find(|(name, _)| *name == &key)
            .map(|(_, item)| item);
        let target_value = target
            .custom_data
            .iter()
            .find(|(name, _)| *name == &key)
            .map(|(_, item)| item);
        if source_value == base_value || target_value != base_value {
            continue;
        }
        match source_value {
            Some(item) => target.custom_data.insert(key, item.clone()),
            None => target.custom_data.remove(&key),
        }
    }
}

fn merge_custom_icons_three_way(target: &mut Database, source: &Database, base: &Database) {
    let ids: std::collections::HashSet<_> = base
        .custom_icons
        .keys()
        .chain(source.custom_icons.keys())
        .copied()
        .collect();

    for id in ids {
        let base_icon = base.custom_icons.get(&id);
        let source_icon = source.custom_icons.get(&id);
        let target_icon = target.custom_icons.get(&id);
        let source_changed = source_icon != base_icon;
        let target_changed = target_icon != base_icon;
        if !source_changed || target_changed {
            continue;
        }
        match source_icon {
            Some(icon) => {
                target.custom_icons.insert(id, icon.clone());
            }
            None => {
                target.custom_icons.remove(&id);
            }
        }
    }
}

fn merge_copy<T: Copy + PartialEq>(target: &mut T, source: T, base: T) {
    if *target == base && source != base {
        *target = source;
    }
}

fn merge_clone<T: Clone + PartialEq>(target: &mut T, source: &T, base: &T) {
    if target == base && source != base {
        *target = source.clone();
    }
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
