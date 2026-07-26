use keeless_schema::{EmptyResult, OperationSuccess, UpdateTagStyleArgs};

use crate::features::tag_styles;
use crate::{CoreError, KeelessCore, Result};

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
    let value = serde_json::to_string(&styles)?;
    let timestamp_ms = core.clock.now_millis();
    let mutation = super::mutations::Mutation::UpdateTagStyles {
        value: value.clone(),
        timestamp_ms,
    };
    super::mutations::mutate(core, &mutation, move |database| {
        database
            .custom_data
            .set_at(tag_styles::CUSTOM_DATA_KEY, &value, Some(timestamp_ms));
        database.mark_modified();
    })
    .await?;
    core.touch_activity();
    Ok(EmptyResult {})
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: UpdateTagStyleArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::UpdateTagStyle(run(core, args).await?))
}
