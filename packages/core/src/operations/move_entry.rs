use keeless_schema::{EmptyResult, MoveEntryArgs, OperationSuccess};

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn run(core: &mut KeelessCore, args: MoveEntryArgs) -> Result<EmptyResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }

    let entry_id = parse_node_id(args.entry_id)?;
    let parent_group_id = parse_node_id(args.parent_group_id)?;
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    if handle.database().get_entry(&entry_id).is_none() {
        return Err(CoreError::EntryNotFound);
    }
    if handle.database().get_group(&parent_group_id).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    let Some(current_parent) = handle
        .database()
        .validate_reposition_entry(&entry_id, &parent_group_id)
    else {
        return Err(CoreError::InvalidEntryMove);
    };
    if current_parent == parent_group_id {
        return Ok(EmptyResult {});
    }
    let timestamp_ms = core.clock.now_millis();
    let mutation = super::mutations::Mutation::MoveEntry {
        id: entry_id,
        parent: parent_group_id,
        timestamp_ms,
    };
    super::mutations::mutate(core, &mutation, move |database| {
        let applied = database.reposition_entry_at(
            &entry_id,
            &parent_group_id,
            keeless_kdbx::DateInstant::EpochMillis(timestamp_ms),
        );
        debug_assert!(applied);
    })
    .await?;

    core.touch_activity();
    Ok(EmptyResult {})
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: MoveEntryArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::MoveEntry(run(core, args).await?))
}
