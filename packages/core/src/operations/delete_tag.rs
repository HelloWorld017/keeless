use keeless_schema::{DeleteTagArgs, EmptyResult, OperationSuccess};

use crate::features::tag_styles;
use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn run(core: &mut KeelessCore, args: DeleteTagArgs) -> Result<EmptyResult> {
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    let name = tag_styles::normalize_name(&args.name)?;
    let mut styles = tag_styles::load(handle.database())?;
    if !styles.contains_key(name) {
        return Err(CoreError::TagStyleNotFound);
    }
    if tag_styles::is_used(handle.database(), name) {
        return Err(CoreError::TagInUse);
    }
    styles.remove(name);
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
    args: DeleteTagArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::DeleteTag(run(core, args).await?))
}
