use keeless_schema::{DeleteEntryArgs, EmptyResult, OperationSuccess};

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn run(core: &mut KeelessCore, args: DeleteEntryArgs) -> Result<EmptyResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let entry_id = parse_node_id(args.entry_id)?;
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    let database = handle.database();
    if database.get_entry(&entry_id).is_none() {
        return Err(CoreError::EntryNotFound);
    }
    let in_recycle_bin = database.is_entry_in_recycle_bin(&entry_id);
    if args.permanent && !in_recycle_bin {
        return Err(CoreError::InvalidEntryDelete);
    }
    if !args.permanent && in_recycle_bin {
        return Err(CoreError::InvalidEntryDelete);
    }
    let recycle_bin_id = uuid::Uuid::new_v4();
    if !database.can_delete_entry(&entry_id, args.permanent, recycle_bin_id) {
        return Err(CoreError::InvalidEntryDelete);
    }
    let timestamp_ms = core.clock.now_millis();
    let mutation = super::mutations::Mutation::DeleteEntry {
        id: entry_id,
        permanent: args.permanent,
        recycle_bin_id,
        timestamp_ms,
    };
    super::mutations::mutate(core, &mutation, move |database| {
        let applied =
            database.delete_entry_at(&entry_id, args.permanent, recycle_bin_id, timestamp_ms);
        debug_assert!(applied);
    })
    .await?;
    core.touch_activity();
    Ok(EmptyResult {})
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: DeleteEntryArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::DeleteEntry(run(core, args).await?))
}
