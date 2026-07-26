use keeless_schema::{EmptyResult, OperationSuccess, RenameGroupArgs};

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn run(core: &mut KeelessCore, args: RenameGroupArgs) -> Result<EmptyResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let group_id = parse_node_id(args.group_id)?;
    let name = args.name.trim();
    if name.is_empty() {
        return Err(CoreError::InvalidGroupName);
    }
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    if handle.database().get_group(&group_id).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    let name = name.to_string();
    let timestamp_ms = core.clock.now_millis();
    let mutation = super::mutations::Mutation::RenameGroup {
        id: group_id,
        name: name.clone(),
        timestamp_ms,
    };
    super::mutations::mutate(core, &mutation, move |database| {
        let applied = database.rename_group_at(
            &group_id,
            name,
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
    args: RenameGroupArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::RenameGroup(run(core, args).await?))
}
