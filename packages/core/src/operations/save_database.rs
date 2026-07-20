use keeless_schema::{EmptyResult, OperationSuccess, SaveDatabaseArgs};
use zeroize::Zeroizing;

use crate::{KeelessCore, Result};

pub(crate) async fn run(core: &mut KeelessCore, password: Option<&[u8]>) -> Result<EmptyResult> {
    core.sync(password).await?;
    Ok(EmptyResult {})
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: SaveDatabaseArgs,
) -> Result<OperationSuccess> {
    let password = args
        .password
        .take()
        .map(|password| Zeroizing::new(password.into_bytes()));
    Ok(OperationSuccess::SaveDatabase(
        run(core, password.as_ref().map(|password| password.as_slice())).await?,
    ))
}
