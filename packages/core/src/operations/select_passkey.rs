use keeless_schema::{EmptyResult, OperationSuccess, SelectPasskeyArgs};

use crate::{KeelessCore, PasskeyConsentMode, PasskeyConsentRequest, Result};

pub(crate) async fn run(core: &mut KeelessCore, _: SelectPasskeyArgs) -> Result<EmptyResult> {
    core.request_passkey_consent(PasskeyConsentRequest {
        mode: PasskeyConsentMode::Selection,
        rp_id: "this device".into(),
        accounts: vec!["Use Keeless".into()],
    })
    .await?;
    Ok(EmptyResult {})
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: SelectPasskeyArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::SelectPasskey(run(core, args).await?))
}
