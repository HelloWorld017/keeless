use keeless_schema::{OperationSuccess, UpgradeArgs, UpgradeResult};

use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn execute(
    core: &mut KeelessCore,
    _args: UpgradeArgs,
) -> Result<OperationSuccess> {
    let sender = core
        .authenticated_sender()
        .ok_or_else(|| CoreError::Host("upgrade requires an authenticated wire sender".into()))?;
    let recipient = core.upgrade_sender(&sender).await?;
    Ok(OperationSuccess::Upgrade(UpgradeResult {
        public_key: recipient,
    }))
}
