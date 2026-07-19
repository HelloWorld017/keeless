use keeless_schema::{GetGroupHierarchyArgs, GroupHierarchyResult, OperationSuccess};

use crate::model::group_hierarchy;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore) -> Result<GroupHierarchyResult> {
    core.enforce_auto_lock();
    let result = {
        let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
        group_hierarchy(handle.database())?
    };
    core.touch_activity();
    Ok(result)
}

pub(super) fn execute(
    core: &mut KeelessCore,
    _args: GetGroupHierarchyArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetGroupHierarchy(run(core)?))
}
