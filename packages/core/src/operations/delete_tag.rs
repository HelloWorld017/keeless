use keeless_schema::{DeleteTagArgs, EmptyResult, OperationSuccess};

use crate::features::tag_styles;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: DeleteTagArgs) -> Result<EmptyResult> {
    core.enforce_auto_lock();
    let handle = core.handle.as_mut().ok_or(CoreError::DatabaseLocked)?;
    let name = tag_styles::normalize_name(&args.name)?;
    let mut styles = tag_styles::load(handle.database())?;
    if !styles.contains_key(name) {
        return Err(CoreError::TagStyleNotFound);
    }
    if tag_styles::is_used(handle.database(), name) {
        return Err(CoreError::TagInUse);
    }
    styles.remove(name);
    handle.apply_update(|database| {
        tag_styles::store(database, &styles)?;
        Ok::<_, CoreError>(true)
    })?;
    core.touch_activity();
    Ok(EmptyResult {})
}

pub(super) fn execute(core: &mut KeelessCore, args: DeleteTagArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::DeleteTag(run(core, args)?))
}
