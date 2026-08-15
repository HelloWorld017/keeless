use keeless_schema::{
    DeleteRecentDatabaseArgs, EmptyResult, GetRecentDatabasesArgs, OpenRecentDatabaseArgs,
    OperationSuccess, RecentDatabase, RecentDatabasesResult,
};
use serde::{Deserialize, Serialize};

use crate::{CoreError, KeelessCore, Result, Selection};

const RECENT_STATE_VERSION: u8 = 1;
const MAX_RECENT_DATABASES: usize = 5;

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecentState {
    version: u8,
    databases: Vec<RecentDatabase>,
}

async fn load(core: &KeelessCore) -> Result<RecentState> {
    let Some(bytes) = core
        .core_state
        .load()
        .await
        .map_err(|error| CoreError::Host(error.to_string()))?
    else {
        return Ok(RecentState {
            version: RECENT_STATE_VERSION,
            databases: Vec::new(),
        });
    };
    let state: RecentState = serde_json::from_slice(&bytes)
        .map_err(|error| CoreError::InvalidConfig(error.to_string()))?;
    if state.version != RECENT_STATE_VERSION || state.databases.len() > MAX_RECENT_DATABASES {
        return Err(CoreError::InvalidConfig(
            "invalid recent database state".into(),
        ));
    }
    Ok(state)
}

async fn save(core: &KeelessCore, state: &RecentState) -> Result<()> {
    let bytes = serde_json::to_vec(state)?;
    core.core_state
        .save(&bytes)
        .await
        .map_err(|error| CoreError::Host(error.to_string()))
}

pub(crate) async fn record_success(core: &KeelessCore) -> Result<()> {
    let Some(selection) = &core.selection else {
        return Ok(());
    };
    let Some(storage_config) = &selection.storage_config else {
        return Ok(());
    };
    let name = core
        .handle
        .as_ref()
        .map(|handle| handle.database().name.clone())
        .unwrap_or_default();
    let record = RecentDatabase {
        id: selection.database_id.recent_id(),
        name,
        storage_type: storage_config.storage_type().into(),
        last_opened_at_ms: core.clock.now_millis(),
    };
    let mut state = load(core).await?;
    state.databases.retain(|database| database.id != record.id);
    state.databases.insert(0, record);
    state.databases.truncate(MAX_RECENT_DATABASES);
    save(core, &state).await
}

pub(super) async fn get(
    core: &mut KeelessCore,
    _args: GetRecentDatabasesArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetRecentDatabases(
        RecentDatabasesResult {
            databases: load(core).await?.databases,
        },
    ))
}

pub(super) async fn open(
    core: &mut KeelessCore,
    args: OpenRecentDatabaseArgs,
) -> Result<OperationSuccess> {
    let database_id = crate::DatabaseId::from_recent_id(&args.id)?;
    if !load(core)
        .await?
        .databases
        .iter()
        .any(|database| database.id == args.id)
    {
        return Err(CoreError::InvalidRecentDatabase);
    }
    super::lock::run(core);
    core.selection = None;
    core.persistence.select_by_id(&database_id).await?;
    core.selection = Some(Selection {
        descriptor: None,
        provider: None,
        database_id,
        exists: true,
        storage_config: None,
    });
    core.sync_status = crate::SyncStatus::Idle;
    core.sync_error = None;
    core.dirty = false;
    Ok(OperationSuccess::OpenRecentDatabase(EmptyResult {}))
}

pub(super) async fn delete(
    core: &mut KeelessCore,
    args: DeleteRecentDatabaseArgs,
) -> Result<OperationSuccess> {
    let database_id = crate::DatabaseId::from_recent_id(&args.id)?;
    if core
        .selection
        .as_ref()
        .is_some_and(|selection| selection.database_id == database_id)
    {
        return Err(CoreError::RecentDatabaseSelected);
    }
    let mut state = load(core).await?;
    let original_len = state.databases.len();
    state.databases.retain(|database| database.id != args.id);
    if state.databases.len() == original_len {
        return Err(CoreError::InvalidRecentDatabase);
    }
    core.persistence.purge(&database_id).await?;
    save(core, &state).await?;
    Ok(OperationSuccess::DeleteRecentDatabase(EmptyResult {}))
}
