use crate::{
    CoreError, KeelessCore, Result, credential::CredentialVault,
    extensions::password_session::PasswordSessionExtension,
};
use keeless_kdbx::{
    CompositeCredentials, Database, DatabaseVersion, Group, IconImage, IconImageStandard, NodeId,
    get_builtin_templates, initialize_database_key,
};
use keeless_schema::{CreateArgs, EmptyResult, OperationSuccess};
use keeless_sync::{FileHandle, StorageErrorKind, SyncError, SyncOptions};

pub(crate) async fn run(core: &mut KeelessCore, password: &[u8]) -> Result<()> {
    let (provider, path, database_id) = core
        .selection
        .as_ref()
        .map(|selection| {
            (
                selection.storage.as_ref().map(|storage| storage.provider()),
                selection
                    .descriptor
                    .as_ref()
                    .map(|descriptor| descriptor.path.clone()),
                selection.database_id.clone(),
            )
        })
        .ok_or(CoreError::NoDatabaseSelected)?;
    let provider = provider.ok_or(CoreError::RecentDatabaseUnavailable)?;
    let path = path.ok_or(CoreError::RecentDatabaseUnavailable)?;

    if provider.stat(&path).await?.is_some() {
        mark_existing(core);
        return Err(CoreError::DatabaseAlreadyExists);
    }

    core.persistence
        .quarantine_journal("journal predates newly created database")
        .await?;
    core.persistence
        .quarantine_cache("cache predates newly created database")
        .await?;
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
    let credentials = CompositeCredentials::new().with_password(password)?;
    let key = initialize_database_key(&mut database, &credentials)?;
    let journal = super::mutations::MutationCoordinator::new(&key, database_id, 0)?;

    let handle =
        match FileHandle::create(provider, path, database, &key, SyncOptions::default()).await {
            Ok(handle) => handle,
            Err(SyncError::Storage(error)) if error.kind() == StorageErrorKind::AlreadyExists => {
                mark_existing(core);
                return Err(CoreError::DatabaseAlreadyExists);
            }
            Err(error) => return Err(error.into()),
        };
    let cache = journal.encode_cache(
        handle.checkpoint_bytes(),
        handle
            .database()
            .kdf_parameters
            .as_ref()
            .ok_or(CoreError::Crypto)?,
    )?;
    core.persistence.write_cache(&cache).await?;
    core.persistence.clear_journal().await?;
    if let Err(error) = core.activate_database_state(&key).await {
        core.drop_core_server();
        core.encrypted_state = None;
        return Err(error);
    }
    if core.selection.as_ref().is_some_and(|selection| {
        selection
            .storage
            .as_ref()
            .is_some_and(|storage| storage.is_persistent())
    }) {
        core.persist().await?;
    }
    let credential = if core.settings.paranoia_mode {
        None
    } else {
        Some(CredentialVault::wrap(&key)?)
    };
    core.extensions.unlock(handle.database(), &key)?;
    core.credential = credential;
    core.handle = Some(handle);
    core.journal = Some(journal);
    core.reset_sync_state();
    core.set_sync_state(crate::SyncStatus::Idle, None);
    core.set_sync_dirty(false);
    mark_existing(core);
    core.last_activity_ms = Some(core.clock.monotonic_millis());
    crate::recent::record_success(core).await?;
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
    let password_argument = {
        let now_millis = core.clock.monotonic_millis();
        core.extensions
            .get_mut::<PasswordSessionExtension>()
            .resolve_argument(
                args.password.take(),
                args.password_session.take(),
                now_millis,
            )?
    };
    let password = match password_argument {
        Some(password) => password,
        None => {
            core.request_password(crate::PasswordInputMode::Create)
                .await?
        }
    };
    run(core, &password).await?;
    Ok(OperationSuccess::Create(EmptyResult {}))
}
