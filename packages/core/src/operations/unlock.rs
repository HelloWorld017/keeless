use std::sync::Arc;

use keeless_kdbx::CompositeKey;
use keeless_schema::{EmptyResult, OperationSuccess, UnlockArgs};
use keeless_sync::{FileHandle, SyncError, SyncOptions};
use zeroize::Zeroizing;

use crate::{CoreError, KeelessCore, Result, credential::CredentialVault};

pub(crate) async fn run(core: &mut KeelessCore, password: &[u8]) -> Result<()> {
    let (provider, path) = core
        .selection
        .as_ref()
        .map(|selection| {
            (
                Arc::clone(&selection.provider),
                selection.descriptor.path.clone(),
            )
        })
        .ok_or(CoreError::NoDatabaseSelected)?;
    let exists = provider.stat(&path).await?.is_some();
    if !exists {
        mark_missing_if_locked(core);
        return Err(CoreError::DatabaseNotFound);
    }
    let key = CompositeKey::new().with_password(password)?;
    let handle = match FileHandle::open(provider, path, &key, SyncOptions::default()).await {
        Ok(handle) => handle,
        Err(SyncError::RemoteNotFound(_)) => {
            mark_missing_if_locked(core);
            return Err(CoreError::DatabaseNotFound);
        }
        Err(error) => return Err(error.into()),
    };
    let raw_key = key.build_raw_key()?;
    let credential = if core.settings.paranoia_mode {
        None
    } else {
        Some(CredentialVault::wrap(&raw_key)?)
    };
    core.handle = None;
    core.credential = credential;
    core.handle = Some(handle);
    if let Some(selection) = &mut core.selection {
        selection.exists = true;
    }
    core.last_activity_ms = Some(core.clock.monotonic_millis());
    Ok(())
}

fn mark_missing_if_locked(core: &mut KeelessCore) {
    if core.handle.is_none() {
        if let Some(selection) = &mut core.selection {
            selection.exists = false;
        }
    }
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: UnlockArgs,
) -> Result<OperationSuccess> {
    let password = Zeroizing::new(std::mem::take(&mut args.password).into_bytes());
    run(core, &password).await?;
    Ok(OperationSuccess::Unlock(EmptyResult {}))
}
