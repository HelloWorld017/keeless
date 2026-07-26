use keeless_schema::{ConfigResult, GetConfigArgs, OperationSuccess};

use crate::{KeelessConfig, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore) -> KeelessConfig {
    core.touch_activity();
    core.settings.clone()
}

pub(super) fn execute(core: &mut KeelessCore, _args: GetConfigArgs) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetConfig(ConfigResult {
        config: run(core),
    }))
}
