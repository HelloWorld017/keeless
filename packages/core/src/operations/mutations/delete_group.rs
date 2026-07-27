use keeless_kdbx::{Database, NodeId};
use keeless_schema::{DeleteGroupArgs, EmptyResult, OperationSuccess};
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
        .delete_group_at(
            &mutation.id,
            mutation.permanent,
            mutation.recycle_bin_id,
            mutation.timestamp_ms,
        )
        .then_some(())
        .ok_or(CoreError::InvalidJournal)
}

pub(crate) async fn run(core: &mut KeelessCore, args: DeleteGroupArgs) -> Result<EmptyResult> {
    let id = parse_node_id(args.group_id)?;
    let database = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database();
    if database.get_group(&id).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    if database.root_group_id == Some(id) || database.is_recycle_bin(&id) {
        return Err(CoreError::InvalidGroupDelete);
    }
    let payload = Mutation {
        id,
        permanent: false,
        recycle_bin_id: Uuid::new_v4(),
        timestamp_ms: core.clock.now_millis(),
    };
    if !database.can_delete_group(&id, false, payload.recycle_bin_id) {
        return Err(CoreError::InvalidGroupDelete);
    }
    let mutation = JournalMutation::DeleteGroup(payload.clone());
    mutate(core, &mutation, move |database| {
        apply(database, &payload).expect("validated delete group")
    })
    .await?;
    core.touch_activity();
    Ok(EmptyResult {})
}

pub(crate) async fn execute(
    core: &mut KeelessCore,
    args: DeleteGroupArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::DeleteGroup(run(core, args).await?))
}
