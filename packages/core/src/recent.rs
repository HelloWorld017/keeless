use keeless_schema::RecentDatabase;
use serde::{Deserialize, Serialize};

use crate::{CoreError, KeelessCore, Result};

const RECENT_STATE_VERSION: u8 = 2;

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RecentState {
    version: u8,
    pub(crate) databases: Vec<RecentDatabase>,
}

pub(crate) async fn load(core: &KeelessCore) -> Result<RecentState> {
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
    let Ok(state) = serde_json::from_slice::<RecentState>(&bytes) else {
        return Ok(RecentState {
            version: RECENT_STATE_VERSION,
            databases: Vec::new(),
        });
    };
    if state.version != RECENT_STATE_VERSION {
        return Ok(RecentState {
            version: RECENT_STATE_VERSION,
            databases: Vec::new(),
        });
    }
    Ok(state)
}

pub(crate) async fn save(core: &KeelessCore, state: &RecentState) -> Result<()> {
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
    let Some(storage) = &selection.storage else {
        return Ok(());
    };
    if !storage.is_persistent() {
        return Ok(());
    }
    let Some(descriptor) = &selection.descriptor else {
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
        storage_type: descriptor.provider.clone(),
        descriptor: descriptor.clone(),
        last_opened_at_ms: core.clock.now_millis(),
    };
    let mut state = load(core).await?;
    state.databases.retain(|database| database.id != record.id);
    state.databases.insert(0, record);
    save(core, &state).await
}
