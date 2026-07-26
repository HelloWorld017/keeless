use keeless_schema::{DeleteGroupArgs, EmptyResult, OperationSuccess};

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn run(core: &mut KeelessCore, args: DeleteGroupArgs) -> Result<EmptyResult> {
    let group_id = parse_node_id(args.group_id)?;
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    let database = handle.database();

    if database.get_group(&group_id).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    if database.root_group_id == Some(group_id) || database.is_recycle_bin(&group_id) {
        return Err(CoreError::InvalidGroupDelete);
    }

    let recycle_bin_id = uuid::Uuid::new_v4();
    if !database.can_delete_group(&group_id, false, recycle_bin_id) {
        return Err(CoreError::InvalidGroupDelete);
    }
    let timestamp_ms = core.clock.now_millis();
    let mutation = super::mutations::Mutation::DeleteGroup {
        id: group_id,
        permanent: false,
        recycle_bin_id,
        timestamp_ms,
    };
    super::mutations::mutate(core, &mutation, move |database| {
        let applied = database.delete_group_at(&group_id, false, recycle_bin_id, timestamp_ms);
        debug_assert!(applied);
    })
    .await?;

    core.touch_activity();
    Ok(EmptyResult {})
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: DeleteGroupArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::DeleteGroup(run(core, args).await?))
}
