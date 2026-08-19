use keeless_schema::{EmptyResult, OperationSuccess, RevokePasswordSessionArgs};

use crate::{KeelessCore, Result, extensions::password_session::PasswordSessionExtension};

pub(super) fn execute(
    core: &mut KeelessCore,
    _: RevokePasswordSessionArgs,
) -> Result<OperationSuccess> {
    core.extensions
        .get_mut::<PasswordSessionExtension>()
        .revoke();
    Ok(OperationSuccess::RevokePasswordSession(EmptyResult {}))
}
