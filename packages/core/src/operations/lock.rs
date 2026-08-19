use keeless_schema::{EmptyResult, LockArgs, OperationSuccess};

use crate::{KeelessCore, Result, extensions::password_session::PasswordSessionExtension};

pub(crate) fn run(core: &mut KeelessCore) {
    core.clear_transfers();
    core.drop_core_server();
    core.encrypted_state = None;
    core.settings = keeless_schema::KeelessConfig::default();
    core.extensions.lock();
    core.handle = None;
    core.credential = None;
    core.last_activity_ms = None;
    core.journal = None;
    core.reset_sync_state();
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: LockArgs,
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
    if core.handle.is_some() {
        core.sync(password.as_ref().map(|password| password.as_slice()))
            .await?;
    }
    run(core);
    Ok(OperationSuccess::Lock(EmptyResult {}))
}
