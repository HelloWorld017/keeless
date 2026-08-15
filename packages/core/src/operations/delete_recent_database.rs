use keeless_schema::{DeleteRecentDatabaseArgs, EmptyResult, OperationSuccess};

use crate::{CoreError, DatabaseId, KeelessCore, Result};

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: DeleteRecentDatabaseArgs,
) -> Result<OperationSuccess> {
    let database_id = DatabaseId::from_recent_id(&args.id)?;
    let selected = core
        .selection
        .as_ref()
        .is_some_and(|selection| selection.database_id == database_id);
    if selected && core.handle.is_some() {
        return Err(CoreError::RecentDatabaseSelected);
    }
    let mut state = crate::recent::load(core).await?;
    let original_len = state.databases.len();
    state.databases.retain(|database| database.id != args.id);
    if state.databases.len() == original_len {
        return Err(CoreError::InvalidRecentDatabase);
    }
    core.persistence.purge(&database_id).await?;
    if selected {
        core.selection = None;
    }
    crate::recent::save(core, &state).await?;
    Ok(OperationSuccess::DeleteRecentDatabase(EmptyResult {}))
}
