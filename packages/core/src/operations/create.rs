use std::sync::Arc;

use keeless_kdbx::{
    CompositeKey, Database, DatabaseVersion, Group, IconImage, IconImageStandard, NodeId,
    get_builtin_templates,
};
use keeless_schema::{CreateArgs, EmptyResult, OperationSuccess};
use keeless_sync::{FileHandle, StorageErrorKind, SyncError, SyncOptions};
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

    if provider.stat(&path).await?.is_some() {
        mark_existing(core);
        return Err(CoreError::DatabaseAlreadyExists);
    }

    let key = CompositeKey::new().with_password(password)?;
    let raw_key = key.build_raw_key()?;
    let credential = if core.settings.paranoia_mode {
        None
    } else {
        Some(CredentialVault::wrap(&raw_key)?)
    };
    let mut database = Database::new(DatabaseVersion::KDBX4);
    let root_id = NodeId::new_uuid();
    let mut root = Group::new(root_id);
    root.title = "Root".into();
    database.groups.insert(root_id, root);
    database.root_group_id = Some(root_id);

    let templates_id = NodeId::new_uuid();
    let mut templates = Group::new(templates_id);
    templates.title = "Templates".into();
    templates.icon = IconImage::Standard(IconImageStandard::new(48));
    templates.enable_searching = false;
    database.add_group(templates, &root_id);
    database.entry_templates_uuid = templates_id.as_uuid().copied();
    for template in get_builtin_templates() {
        database.add_entry(
            keeless_kdbx::kdbx::template::builtin_entry(template),
            &templates_id,
        );
    }

    let handle =
        match FileHandle::create(provider, path, database, &key, SyncOptions::default()).await {
            Ok(handle) => handle,
            Err(SyncError::Storage(error)) if error.kind() == StorageErrorKind::AlreadyExists => {
                mark_existing(core);
                return Err(CoreError::DatabaseAlreadyExists);
            }
            Err(error) => return Err(error.into()),
        };
    core.handle = None;
    core.credential = credential;
    core.handle = Some(handle);
    mark_existing(core);
    core.last_activity_ms = Some(core.clock.monotonic_millis());
    Ok(())
}

fn mark_existing(core: &mut KeelessCore) {
    if let Some(selection) = &mut core.selection {
        selection.exists = true;
    }
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: CreateArgs,
) -> Result<OperationSuccess> {
    let password = Zeroizing::new(std::mem::take(&mut args.password).into_bytes());
    run(core, &password).await?;
    Ok(OperationSuccess::Create(EmptyResult {}))
}
