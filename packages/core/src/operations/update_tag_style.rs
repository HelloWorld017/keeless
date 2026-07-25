use keeless_schema::{EmptyResult, OperationSuccess, UpdateTagStyleArgs};

use crate::features::tag_styles;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: UpdateTagStyleArgs) -> Result<EmptyResult> {
    core.enforce_auto_lock();
    let handle = core.handle.as_mut().ok_or(CoreError::DatabaseLocked)?;
    let name = tag_styles::normalize_name(&args.name)?.to_string();
    let mut style = args.style;
    tag_styles::normalize_style(handle.database(), &mut style)?;
    let mut styles = tag_styles::load(handle.database())?;
    if styles.get(&name) == Some(&style) {
        return Ok(EmptyResult {});
    }
    styles.insert(name, style);
    handle.apply_update(|database| {
        tag_styles::store(database, &styles)?;
        Ok::<_, CoreError>(true)
    })?;
    core.touch_activity();
    Ok(EmptyResult {})
}

pub(super) fn execute(
    core: &mut KeelessCore,
    args: UpdateTagStyleArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::UpdateTagStyle(run(core, args)?))
}
