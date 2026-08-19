use keeless_schema::{CreatePasswordSessionArgs, CreatePasswordSessionResult, OperationSuccess};
use zeroize::Zeroizing;

use crate::{
    KeelessCore, PasswordInputMode, Result, extensions::password_session::PasswordSessionExtension,
};

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: CreatePasswordSessionArgs,
) -> Result<OperationSuccess> {
    let password = match args.password.take() {
        Some(password) => Zeroizing::new(password.into_bytes()),
        None => core.request_password(PasswordInputMode::Session).await?,
    };
    let now_millis = core.clock.monotonic_millis();
    let password_session = core
        .extensions
        .get_mut::<PasswordSessionExtension>()
        .create(&password, now_millis)?;
    Ok(OperationSuccess::CreatePasswordSession(
        CreatePasswordSessionResult { password_session },
    ))
}
