use keeless_schema::{OperationSuccess, UpgradeArgs, UpgradeResult};

use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn execute(
    core: &mut KeelessCore,
    _args: UpgradeArgs,
) -> Result<OperationSuccess> {
    let sender = core
        .authenticated_sender
        .clone()
        .ok_or_else(|| CoreError::Host("upgrade requires an authenticated wire sender".into()))?;
    let server = core.core_server.as_mut().ok_or(CoreError::DatabaseLocked)?;
    let recipient = server.public_key_bundle();
    match sender.approval {
        keeless_lesswire::SenderApproval::Runtime => {
            server.add_runtime_approval(&sender.public_key_bundle)
        }
        keeless_lesswire::SenderApproval::Persisted => {
            server.approve_upgrade(&sender.public_key_bundle).await
        }
    }
    .map_err(|error| CoreError::Host(error.to_string()))?;
    Ok(OperationSuccess::Upgrade(UpgradeResult {
        public_key: recipient,
    }))
}
