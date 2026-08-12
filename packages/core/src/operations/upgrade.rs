use keeless_schema::{OperationSuccess, UpgradeArgs, UpgradeResult};

use crate::{ConnectionApprovalKind, ConnectionApprovalRequest, CoreError, KeelessCore, Result};

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
    let approved = core
        .connection_approval
        .approve_connection(ConnectionApprovalRequest {
            sender: sender.public_key_bundle.clone(),
            sender_scope: sender.scope,
            recipient: recipient.clone(),
            recipient_scope: keeless_lesswire::KeyScope::Core,
            kind: ConnectionApprovalKind::Upgrade,
        })
        .await?;
    if !approved {
        return Err(CoreError::Host("database access was not approved".into()));
    }
    server
        .add_persisted_approval(&sender.public_key_bundle)
        .await
        .map_err(|error| CoreError::Host(error.to_string()))?;
    Ok(OperationSuccess::Upgrade(UpgradeResult {
        public_key: recipient,
    }))
}
