use base64::{Engine as _, engine::general_purpose::STANDARD};
use keeless_schema::{CustomIcon, CustomIconsResult, GetCustomIconsArgs, OperationSuccess};

use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore) -> Result<CustomIconsResult> {
    core.enforce_auto_lock();
    let mut icons = {
        let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
        handle
            .database()
            .custom_icons
            .values()
            .map(|icon| CustomIcon {
                uuid: icon.uuid.hyphenated().to_string(),
                data_base64: STANDARD.encode(&icon.data),
                name: icon.name.clone(),
                last_modification_time_ms: icon.last_modification_time,
            })
            .collect::<Vec<_>>()
    };
    icons.sort_by(|left, right| left.uuid.cmp(&right.uuid));
    core.touch_activity();
    Ok(CustomIconsResult { icons })
}

pub(super) fn execute(
    core: &mut KeelessCore,
    _args: GetCustomIconsArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetCustomIcons(run(core)?))
}
