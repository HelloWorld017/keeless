use keeless_kdbx::{Database, NodeId};
use keeless_schema::{DeleteEntryArgs, EmptyResult, OperationSuccess};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Mutation as JournalMutation, mutate};
use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Mutation {
    pub(super) id: NodeId,
    pub(super) permanent: bool,
    pub(super) recycle_bin_id: Uuid,
    pub(super) timestamp_ms: i64,
}

pub(super) fn apply(database: &mut Database, mutation: &Mutation) -> Result<()> {
    database
        .delete_entry_at(
            &mutation.id,
            mutation.permanent,
            mutation.recycle_bin_id,
            mutation.timestamp_ms,
        )
        .then_some(())
        .ok_or(CoreError::InvalidJournal)
}

pub(crate) async fn run(core: &mut KeelessCore, args: DeleteEntryArgs) -> Result<EmptyResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let id = parse_node_id(args.entry_id)?;
    let database = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database();
    if database.get_entry(&id).is_none() {
        return Err(CoreError::EntryNotFound);
    }
    let in_recycle_bin = database.is_entry_in_recycle_bin(&id);
    if args.permanent == !in_recycle_bin {
        return Err(CoreError::InvalidEntryDelete);
    }
    let payload = Mutation {
        id,
        permanent: args.permanent,
        recycle_bin_id: Uuid::new_v4(),
        timestamp_ms: core.clock.now_millis(),
    };
    if !database.can_delete_entry(&id, payload.permanent, payload.recycle_bin_id) {
        return Err(CoreError::InvalidEntryDelete);
    }
    let mutation = JournalMutation::DeleteEntry(payload.clone());
    mutate(core, &mutation, move |database| {
        apply(database, &payload).expect("validated delete entry")
    })
    .await?;
    core.touch_activity();
    Ok(EmptyResult {})
}

pub(crate) async fn execute(
    core: &mut KeelessCore,
    args: DeleteEntryArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::DeleteEntry(run(core, args).await?))
}
