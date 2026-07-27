use keeless_schema::{DeleteTagArgs, EmptyResult, OperationSuccess};

use super::update_tag_style;
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
    update_tag_style::apply_final_state(core, serde_json::to_string(&styles)?).await
}

pub(crate) async fn execute(
    core: &mut KeelessCore,
    args: DeleteTagArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::DeleteTag(run(core, args).await?))
}
