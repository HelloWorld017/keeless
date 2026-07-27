use keeless_kdbx::{Database, DateInstant, NodeId};
use keeless_schema::{EmptyResult, OperationSuccess, RenameGroupArgs};
use serde::{Deserialize, Serialize};

use super::{Mutation as JournalMutation, mutate};
use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Mutation {
    pub(super) id: NodeId,
    pub(super) name: String,
    pub(super) timestamp_ms: i64,
}

pub(super) fn apply(database: &mut Database, mutation: &Mutation) -> Result<()> {
    database
        .rename_group_at(
            &mutation.id,
            mutation.name.clone(),
            DateInstant::EpochMillis(mutation.timestamp_ms),
        )
        .then_some(())
        .ok_or(CoreError::InvalidJournal)
}

pub(crate) async fn run(core: &mut KeelessCore, args: RenameGroupArgs) -> Result<EmptyResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
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
    let payload = Mutation {
        id,
        name: name.to_string(),
        timestamp_ms: core.clock.now_millis(),
    };
    let mutation = JournalMutation::RenameGroup(payload.clone());
    mutate(core, &mutation, move |database| {
        apply(database, &payload).expect("validated rename group")
    })
    .await?;
    core.touch_activity();
    Ok(EmptyResult {})
}

pub(crate) async fn execute(
    core: &mut KeelessCore,
    args: RenameGroupArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::RenameGroup(run(core, args).await?))
}
