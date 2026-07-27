use keeless_kdbx::{Database, DateInstant, IconImage, IconImageStandard, IconUpdate, NodeId};
use keeless_schema::{EmptyResult, OperationSuccess, UpdateGroupArgs};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Mutation as JournalMutation, mutate};
use crate::model::{parse_icon_reference, parse_node_id};
use crate::{CoreError, KeelessCore, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Mutation {
    pub(super) id: NodeId,
    pub(super) name: String,
    pub(super) standard_icon: u32,
    pub(super) custom_icon: Option<Uuid>,
    pub(super) timestamp_ms: i64,
}

pub(super) fn apply(database: &mut Database, mutation: &Mutation) -> Result<()> {
    database
        .update_group_at(
            &mutation.id,
            mutation.name.clone(),
            IconUpdate {
                standard_id: mutation.standard_icon,
                custom_uuid: mutation.custom_icon,
            },
            DateInstant::EpochMillis(mutation.timestamp_ms),
        )
        .then_some(())
        .ok_or(CoreError::InvalidJournal)
}

pub(crate) async fn run(core: &mut KeelessCore, args: UpdateGroupArgs) -> Result<EmptyResult> {
    let id = parse_node_id(args.group_id)?;
    let name = args.name.trim();
    if name.is_empty() {
        return Err(CoreError::InvalidGroupName);
    }
    let database = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database();
    if database.get_group(&id).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    let icon = parse_icon_reference(database, &args.icon)?;
    let group = database.get_group(&id).expect("checked");
    let target_icon = IconImage::Standard(IconImageStandard::new(icon.standard_id));
    if group.title == name
        && group.icon == target_icon
        && group.custom_icon_uuid == icon.custom_uuid
    {
        return Ok(EmptyResult {});
    }
    let payload = Mutation {
        id,
        name: name.to_string(),
        standard_icon: icon.standard_id,
        custom_icon: icon.custom_uuid,
        timestamp_ms: core.clock.now_millis(),
    };
    let mutation = JournalMutation::UpdateGroup(payload.clone());
    mutate(core, &mutation, move |database| {
        apply(database, &payload).expect("validated update group")
    })
    .await?;
    core.touch_activity();
    Ok(EmptyResult {})
}

pub(crate) async fn execute(
    core: &mut KeelessCore,
    args: UpdateGroupArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::UpdateGroup(run(core, args).await?))
}
