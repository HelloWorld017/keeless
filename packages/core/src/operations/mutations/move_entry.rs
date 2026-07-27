use keeless_kdbx::{Database, DateInstant, NodeId};
use keeless_schema::{EmptyResult, MoveEntryArgs, OperationSuccess};
use serde::{Deserialize, Serialize};

use super::{Mutation as JournalMutation, mutate};
use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Mutation {
    pub(super) id: NodeId,
    pub(super) parent: NodeId,
    pub(super) timestamp_ms: i64,
}

pub(super) fn apply(database: &mut Database, mutation: &Mutation) -> Result<()> {
    database
        .reposition_entry_at(
            &mutation.id,
            &mutation.parent,
            DateInstant::EpochMillis(mutation.timestamp_ms),
        )
        .then_some(())
        .ok_or(CoreError::InvalidJournal)
}

pub(crate) async fn run(core: &mut KeelessCore, args: MoveEntryArgs) -> Result<EmptyResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let id = parse_node_id(args.entry_id)?;
    let parent = parse_node_id(args.parent_group_id)?;
    let database = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database();
    if database.get_entry(&id).is_none() {
        return Err(CoreError::EntryNotFound);
    }
    if database.get_group(&parent).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    let Some(current_parent) = database.validate_reposition_entry(&id, &parent) else {
        return Err(CoreError::InvalidEntryMove);
    };
    if current_parent == parent {
        return Ok(EmptyResult {});
    }
    let payload = Mutation {
        id,
        parent,
        timestamp_ms: core.clock.now_millis(),
    };
    let mutation = JournalMutation::MoveEntry(payload.clone());
    mutate(core, &mutation, move |database| {
        apply(database, &payload).expect("validated move entry")
    })
    .await?;
    core.touch_activity();
    Ok(EmptyResult {})
}

pub(crate) async fn execute(
    core: &mut KeelessCore,
    args: MoveEntryArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::MoveEntry(run(core, args).await?))
}
