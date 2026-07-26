use keeless_schema::{EmptyResult, OperationSuccess, SetConfigArgs};

use crate::{KeelessConfigPatch, KeelessCore, Result};

pub(crate) async fn run(core: &mut KeelessCore, patch: KeelessConfigPatch) -> Result<()> {
    let previous = core.settings.clone();
    if let Some(timeout) = patch.auto_lock_timeout_ms {
        core.settings.auto_lock_timeout_ms = timeout;
    }
    if let Some(paranoia) = patch.paranoia_mode {
        core.settings.paranoia_mode = paranoia;
    }
    if let Err(error) = core.persist().await {
        core.settings = previous;
        return Err(error);
    }
    if core.settings.paranoia_mode {
        core.credential = None;
    } else if previous.paranoia_mode && core.credential.is_none() {
        super::lock::run(core);
    }
    core.touch_activity();
    Ok(())
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: SetConfigArgs,
) -> Result<OperationSuccess> {
    run(core, args.config).await?;
    Ok(OperationSuccess::SetConfig(EmptyResult {}))
}
