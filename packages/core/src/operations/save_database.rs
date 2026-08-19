use keeless_schema::{EmptyResult, OperationSuccess, SaveDatabaseArgs};

use crate::{KeelessCore, Result, extensions::password_session::PasswordSessionExtension};

pub(crate) async fn run(core: &mut KeelessCore, password: Option<&[u8]>) -> Result<EmptyResult> {
    core.sync(password).await?;
    Ok(EmptyResult {})
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: SaveDatabaseArgs,
) -> Result<OperationSuccess> {
    let password = {
        let now_millis = core.clock.monotonic_millis();
        core.extensions
            .get_mut::<PasswordSessionExtension>()
            .resolve_argument(
                args.password.take(),
                args.password_session.take(),
                now_millis,
            )?
    };
    Ok(OperationSuccess::SaveDatabase(
        run(core, password.as_ref().map(|password| password.as_slice())).await?,
    ))
}
