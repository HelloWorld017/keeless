use keeless_schema::{EmptyResult, OperationSuccess, UpdateGroupArgs};

use crate::model::{parse_icon_reference, parse_node_id};
use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn run(core: &mut KeelessCore, args: UpdateGroupArgs) -> Result<EmptyResult> {
    let group_id = parse_node_id(args.group_id)?;
    let name = args.name.trim();
    if name.is_empty() {
        return Err(CoreError::InvalidGroupName);
    }
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    if handle.database().get_group(&group_id).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    let icon = parse_icon_reference(handle.database(), &args.icon)?;
    let group = handle.database().get_group(&group_id).expect("checked");
    let target_icon =
        keeless_kdbx::IconImage::Standard(keeless_kdbx::IconImageStandard::new(icon.standard_id));
    if group.title == name
        && group.icon == target_icon
        && group.custom_icon_uuid == icon.custom_uuid
    {
        return Ok(EmptyResult {});
    }
    let name = name.to_string();
    let timestamp_ms = core.clock.now_millis();
    let mutation = super::mutations::Mutation::UpdateGroup {
        id: group_id,
        name: name.clone(),
        standard_icon: icon.standard_id,
        custom_icon: icon.custom_uuid,
        timestamp_ms,
    };
    super::mutations::mutate(core, &mutation, move |database| {
        let applied = database.update_group_at(
            &group_id,
            name,
            icon,
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
    args: UpdateGroupArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::UpdateGroup(run(core, args).await?))
}
