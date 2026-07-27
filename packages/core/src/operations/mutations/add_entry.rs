use keeless_kdbx::{Database, DateInstant, Entry, NodeId};
use keeless_schema::{AddEntryArgs, AddEntryResult, OperationSuccess};
use serde::{Deserialize, Serialize};

use super::{Mutation as JournalMutation, mutate};
use crate::model::{node_id, parse_node_id};
use crate::{CoreError, KeelessCore, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Mutation {
    pub(super) parent: NodeId,
    pub(super) id: NodeId,
    pub(super) timestamp_ms: i64,
}

pub(super) fn apply(database: &mut Database, mutation: &Mutation) -> Result<()> {
    if !database.can_add_entry(&mutation.id, &mutation.parent) {
        return Err(CoreError::InvalidJournal);
    }
    database.add_entry_validated(
        Entry::new_at(mutation.id, DateInstant::EpochMillis(mutation.timestamp_ms)),
        &mutation.parent,
    );
    Ok(())
}

pub(crate) async fn run(core: &mut KeelessCore, args: AddEntryArgs) -> Result<AddEntryResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let parent = parse_node_id(args.parent_group_id)?;
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    if handle.database().get_group(&parent).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    let payload = Mutation {
        parent,
        id: NodeId::new_uuid(),
        timestamp_ms: core.clock.now_millis(),
    };
    let id = payload.id;
    let mutation = JournalMutation::AddEntry(payload.clone());
    mutate(core, &mutation, move |database| {
        apply(database, &payload).expect("validated add entry")
    })
    .await?;
    core.touch_activity();
    Ok(AddEntryResult { id: node_id(id) })
}

pub(crate) async fn execute(
    core: &mut KeelessCore,
    args: AddEntryArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::AddEntry(run(core, args).await?))
}
