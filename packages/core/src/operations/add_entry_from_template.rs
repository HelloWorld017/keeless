use keeless_kdbx::{DateInstant, EntryFieldId, NodeId, kdbx::template};
use keeless_schema::{AddEntryFromTemplateArgs, AddEntryResult, OperationSuccess};

use crate::model::{node_id, parse_node_id};
use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn run(
    core: &mut KeelessCore,
    args: AddEntryFromTemplateArgs,
) -> Result<AddEntryResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let parent_group_id = parse_node_id(args.parent_group_id)?;
    let template_entry_id = parse_node_id(args.template_entry_id)?;
    let key = core
        .credential
        .as_ref()
        .map(|credential| credential.restore_key())
        .transpose()?;
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    let database = handle.database();
    if database.get_group(&parent_group_id).is_none() {
        return Err(CoreError::GroupNotFound);
    }
    if !template::is_template(database, &template_entry_id) {
        return Err(CoreError::EntryNotFound);
    }

    let id = NodeId::new_uuid();
    let link_field_id = uuid::Uuid::new_v4();
    let timestamp_ms = core.clock.now_millis();
    let copy_mode = if key.is_some() {
        template::TemplateCopyMode::PreserveProtected
    } else {
        template::TemplateCopyMode::RedactProtected
    };
    let prepared = template::prepare_instantiation_at(
        handle.database(),
        &template_entry_id,
        &parent_group_id,
        id,
        EntryFieldId::Custom(link_field_id),
        DateInstant::EpochMillis(timestamp_ms),
        copy_mode,
        key.as_ref(),
    )?
    .ok_or(CoreError::EntryNotFound)?;
    let mutation = super::mutations::Mutation::AddEntryFromTemplate {
        parent: parent_group_id,
        template: template_entry_id,
        id,
        link_field_id,
        timestamp_ms,
        preserve_protected: key.is_some(),
    };
    super::mutations::mutate(core, &mutation, move |database| {
        template::commit_instantiation(database, prepared);
    })
    .await?;
    core.touch_activity();
    Ok(AddEntryResult { id: node_id(id) })
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: AddEntryFromTemplateArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::AddEntryFromTemplate(
        run(core, args).await?,
    ))
}
