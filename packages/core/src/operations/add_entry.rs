use keeless_kdbx::{DateInstant, Entry, NodeId};
use keeless_schema::{AddEntryArgs, AddEntryResult, OperationSuccess};

use crate::model::{node_id, parse_node_id};
use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn run(core: &mut KeelessCore, args: AddEntryArgs) -> Result<AddEntryResult> {
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
    let mutation = super::mutations::Mutation::AddEntry {
        parent: parent_group_id,
        id,
        timestamp_ms,
    };
    super::mutations::mutate(core, &mutation, move |database| {
        database.add_entry_validated(
            Entry::new_at(id, DateInstant::EpochMillis(timestamp_ms)),
            &parent_group_id,
        );
    })
    .await?;
    core.touch_activity();
    Ok(AddEntryResult { id: node_id(id) })
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: AddEntryArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::AddEntry(run(core, args).await?))
}
