use keeless_schema::{EmptyResult, MoveGroupArgs, OperationSuccess};

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn run(core: &mut KeelessCore, args: MoveGroupArgs) -> Result<EmptyResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }

    let group_id = parse_node_id(args.group_id)?;
    let parent_group_id = parse_node_id(args.parent_group_id)?;
    let destination_index = args
        .destination_index
        .try_into()
        .map_err(|_| CoreError::InvalidGroupMove)?;
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    if handle.database().get_group(&group_id).is_none()
        || handle.database().get_group(&parent_group_id).is_none()
    {
        return Err(CoreError::GroupNotFound);
    }
    if handle
        .database()
        .validate_reposition_group(&group_id, &parent_group_id, destination_index)
        .is_none()
    {
        return Err(CoreError::InvalidGroupMove);
    }
    let timestamp_ms = core.clock.now_millis();
    let mutation = super::mutations::Mutation::MoveGroup {
        id: group_id,
        parent: parent_group_id,
        index: destination_index,
        timestamp_ms,
    };
    super::mutations::mutate(core, &mutation, move |database| {
        let applied = database.reposition_group_at(
            &group_id,
            &parent_group_id,
            destination_index,
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
    args: MoveGroupArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::MoveGroup(run(core, args).await?))
}
