use keeless_kdbx::Database;
use keeless_schema::{EmptyResult, OperationSuccess, UpdateTagStyleArgs};
use serde::{Deserialize, Serialize};

use super::{Mutation as JournalMutation, mutate};
use crate::features::tag_styles;
use crate::{CoreError, KeelessCore, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Mutation {
    pub(super) value: String,
    pub(super) timestamp_ms: i64,
}

pub(super) fn apply(database: &mut Database, mutation: &Mutation) -> Result<()> {
    database.custom_data.set_at(
        tag_styles::CUSTOM_DATA_KEY,
        &mutation.value,
        Some(mutation.timestamp_ms),
    );
    database.mark_modified();
    Ok(())
}

pub(super) async fn apply_final_state(
    core: &mut KeelessCore,
    value: String,
) -> Result<EmptyResult> {
    let payload = Mutation {
        value,
        timestamp_ms: core.clock.now_millis(),
    };
    let mutation = JournalMutation::UpdateTagStyles(payload.clone());
    mutate(core, &mutation, move |database| {
        apply(database, &payload).expect("tag style application is infallible")
    })
    .await?;
    core.touch_activity();
    Ok(EmptyResult {})
}

pub(crate) async fn run(core: &mut KeelessCore, args: UpdateTagStyleArgs) -> Result<EmptyResult> {
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    let name = tag_styles::normalize_name(&args.name)?.to_string();
    let mut style = args.style;
    tag_styles::normalize_style(handle.database(), &mut style)?;
    let mut styles = tag_styles::load(handle.database())?;
    if styles.get(&name) == Some(&style) {
        return Ok(EmptyResult {});
    }
    styles.insert(name, style);
    apply_final_state(core, serde_json::to_string(&styles)?).await
}

pub(crate) async fn execute(
    core: &mut KeelessCore,
    args: UpdateTagStyleArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::UpdateTagStyle(run(core, args).await?))
}
