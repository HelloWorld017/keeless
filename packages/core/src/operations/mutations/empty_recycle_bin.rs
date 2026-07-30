use keeless_kdbx::Database;
use keeless_schema::{EmptyRecycleBinArgs, EmptyResult, OperationSuccess};
use serde::{Deserialize, Serialize};

use super::{Mutation as JournalMutation, mutate};
use crate::{KeelessCore, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Mutation {
    pub(super) timestamp_ms: i64,
}

pub(super) fn apply(database: &mut Database, mutation: &Mutation) -> Result<()> {
    database.empty_recycle_bin_at(mutation.timestamp_ms);
    Ok(())
}

pub(crate) async fn run(core: &mut KeelessCore, _args: EmptyRecycleBinArgs) -> Result<EmptyResult> {
    let payload = Mutation {
        timestamp_ms: core.clock.now_millis(),
    };
    let mutation = JournalMutation::EmptyRecycleBin(payload.clone());
    mutate(core, &mutation, move |database| {
        apply(database, &payload).expect("empty recycle bin mutation is always valid")
    })
    .await?;
    core.touch_activity();
    Ok(EmptyResult {})
}

pub(crate) async fn execute(
    core: &mut KeelessCore,
    args: EmptyRecycleBinArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::EmptyRecycleBin(run(core, args).await?))
}
