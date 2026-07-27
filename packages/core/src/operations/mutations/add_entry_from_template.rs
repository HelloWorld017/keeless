use keeless_kdbx::{
    CompositeKey, Database, DateInstant, EntryFieldId, NodeId,
    kdbx::template::{self, TemplateCopyMode},
};
use keeless_schema::{AddEntryFromTemplateArgs, AddEntryResult, OperationSuccess};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Mutation as JournalMutation, mutate};
use crate::model::{node_id, parse_node_id};
use crate::{CoreError, KeelessCore, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Mutation {
    pub(super) parent: NodeId,
    pub(super) template: NodeId,
    pub(super) id: NodeId,
    pub(super) link_field_id: Uuid,
    pub(super) timestamp_ms: i64,
    pub(super) preserve_protected: bool,
}

pub(super) fn apply(
    database: &mut Database,
    mutation: &Mutation,
    key: &CompositeKey,
) -> Result<()> {
    template::instantiate_at(
        database,
        &mutation.template,
        &mutation.parent,
        mutation.id,
        EntryFieldId::Custom(mutation.link_field_id),
        DateInstant::EpochMillis(mutation.timestamp_ms),
        if mutation.preserve_protected {
            TemplateCopyMode::PreserveProtected
        } else {
            TemplateCopyMode::RedactProtected
        },
        mutation.preserve_protected.then_some(key),
    )?
    .is_some()
    .then_some(())
    .ok_or(CoreError::InvalidJournal)
}

pub(crate) async fn run(
    core: &mut KeelessCore,
    args: AddEntryFromTemplateArgs,
) -> Result<AddEntryResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let parent = parse_node_id(args.parent_group_id)?;
    let source = parse_node_id(args.template_entry_id)?;
    let key = core
        .credential
        .as_ref()
        .map(|credential| credential.restore_key())
        .transpose()?;
    let database = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database();
    if database.get_group(&parent).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    if !template::is_template(database, &source) {
        return Err(CoreError::EntryNotFound);
    }
    let payload = Mutation {
        parent,
        template: source,
        id: NodeId::new_uuid(),
        link_field_id: Uuid::new_v4(),
        timestamp_ms: core.clock.now_millis(),
        preserve_protected: key.is_some(),
    };
    let prepared = template::prepare_instantiation_at(
        database,
        &payload.template,
        &payload.parent,
        payload.id,
        EntryFieldId::Custom(payload.link_field_id),
        DateInstant::EpochMillis(payload.timestamp_ms),
        if payload.preserve_protected {
            TemplateCopyMode::PreserveProtected
        } else {
            TemplateCopyMode::RedactProtected
        },
        key.as_ref(),
    )?
    .ok_or(CoreError::EntryNotFound)?;
    let id = payload.id;
    let mutation = JournalMutation::AddEntryFromTemplate(payload);
    mutate(core, &mutation, move |database| {
        template::commit_instantiation(database, prepared);
    })
    .await?;
    core.touch_activity();
    Ok(AddEntryResult { id: node_id(id) })
}

pub(crate) async fn execute(
    core: &mut KeelessCore,
    args: AddEntryFromTemplateArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::AddEntryFromTemplate(
        run(core, args).await?,
    ))
}
