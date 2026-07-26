use keeless_schema::{GetPasskeysArgs, OperationSuccess, PasskeysResult};

use crate::features::passkeys;
use crate::{CoreError, KeelessCore, PasswordInputMode, Result};

pub(crate) async fn run(core: &mut KeelessCore, args: GetPasskeysArgs) -> Result<PasskeysResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let unlocked = passkeys::unlock(core, PasswordInputMode::Reveal).await?;
    let credentials = passkeys::visible_credentials(core, &unlocked.key, args.rp_id.as_deref())?
        .into_iter()
        .map(|(entry_id, credential)| passkeys::summary(entry_id, &credential))
        .collect();
    core.touch_activity();
    Ok(PasskeysResult { credentials })
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: GetPasskeysArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetPasskeys(run(core, args).await?))
}
