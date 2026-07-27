use keeless_kdbx::{Database, DateInstant, NodeId};
use keeless_schema::{EmptyResult, MoveGroupArgs, OperationSuccess};
use serde::{Deserialize, Serialize};

use super::{Mutation as JournalMutation, mutate};
use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Mutation {
    pub(super) id: NodeId,
    pub(super) parent: NodeId,
    pub(super) index: usize,
    pub(super) timestamp_ms: i64,
}

pub(super) fn apply(database: &mut Database, mutation: &Mutation) -> Result<()> {
    database
        .reposition_group_at(
            &mutation.id,
            &mutation.parent,
            mutation.index,
            DateInstant::EpochMillis(mutation.timestamp_ms),
        )
        .then_some(())
        .ok_or(CoreError::InvalidJournal)
}

pub(crate) async fn run(core: &mut KeelessCore, args: MoveGroupArgs) -> Result<EmptyResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let id = parse_node_id(args.group_id)?;
    let parent = parse_node_id(args.parent_group_id)?;
    let index = args
        .destination_index
        .try_into()
        .map_err(|_| CoreError::InvalidGroupMove)?;
    let database = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database();
    if database.get_group(&id).is_none() || database.get_group(&parent).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    if database
        .validate_reposition_group(&id, &parent, index)
        .is_none()
    {
        return Err(CoreError::InvalidGroupMove);
    }
    let payload = Mutation {
        id,
        parent,
        index,
        timestamp_ms: core.clock.now_millis(),
    };
    let mutation = JournalMutation::MoveGroup(payload.clone());
    mutate(core, &mutation, move |database| {
        apply(database, &payload).expect("validated move group")
    })
    .await?;
    core.touch_activity();
    Ok(EmptyResult {})
}

pub(crate) async fn execute(
    core: &mut KeelessCore,
    args: MoveGroupArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::MoveGroup(run(core, args).await?))
}
