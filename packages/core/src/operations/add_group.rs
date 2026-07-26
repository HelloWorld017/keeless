use keeless_kdbx::{DateInstant, Group, IconImage, IconImageStandard, NodeId};
use keeless_schema::{AddGroupArgs, AddGroupResult, OperationSuccess};

use crate::model::{node_id, parse_node_id};
use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn run(core: &mut KeelessCore, args: AddGroupArgs) -> Result<AddGroupResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let parent_group_id = parse_node_id(args.parent_group_id)?;
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    if handle.database().get_group(&parent_group_id).is_none() {
        return Err(CoreError::GroupNotFound);
    }

    let id = NodeId::new_uuid();
    let timestamp_ms = core.clock.now_millis();
    let mutation = super::mutations::Mutation::AddGroup {
        parent: parent_group_id,
        id,
        timestamp_ms,
    };
    super::mutations::mutate(core, &mutation, move |database| {
        let mut group = Group::new_at(id, DateInstant::EpochMillis(timestamp_ms));
        group.title = "Untitled Group".into();
        group.icon = IconImage::Standard(IconImageStandard::new(48));
        database.add_group_validated(group, &parent_group_id);
    })
    .await?;
    core.touch_activity();
    Ok(AddGroupResult { id: node_id(id) })
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: AddGroupArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::AddGroup(run(core, args).await?))
}
