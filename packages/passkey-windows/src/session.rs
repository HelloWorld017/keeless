//! Authenticated local IPC state for the Windows passkey provider.

#[cfg(windows)]
use base64::Engine as _;
#[cfg(windows)]
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use keeless_host_desktop_shared::client::{CORE_ENDPOINT_ID, UNTRUSTED_ENDPOINT_ID};
use keeless_host_desktop_shared::state::{ClientState, FileStore};
use keeless_host_desktop_shared::{ClientError, CoreClient, DesktopLauncher};
use keeless_lesswire::KeyScope;
use keeless_schema::{
    AssertPasskeyArgs, CreatePasswordSessionArgs, DatabaseStatus, GetConfigArgs, GetCoreStatusArgs,
    GetPasskeysArgs, Operation, OperationSuccess, RegisterPasskeyArgs, UnlockArgs, UpgradeArgs,
};

/// File holding the provider's lesswire identity and pinned desktop host key.
pub const STATE_FILE: &str = "passkey-windows-state.json";

/// A lazily connected Core client, recreated when the desktop host restarts.
pub struct Session {
    state: ClientState,
    client: Option<CoreClient>,
    launcher: Option<DesktopLauncher>,
    password_session: Option<String>,
    paranoia_mode: Option<bool>,
    needs_sync: bool,
}

impl Session {
    pub async fn load() -> Result<Self, SessionError> {
        Self::load_with_launcher(None).await
    }

    /// The COM activation layer supplies a package-resolved launcher in production.
    pub async fn load_with_launcher(
        launcher: Option<DesktopLauncher>,
    ) -> Result<Self, SessionError> {
        let store = FileStore::project(STATE_FILE)?;
        crate::diagnostics::diagnostic!("keeless-passkey-windows: loading passkey session state");
        let state = ClientState::load(store, KeyScope::Passkey).await?;
        crate::diagnostics::initialize();
        crate::diagnostics::diagnostic!(
            "keeless-passkey-windows: passkey session state loaded (paired={})",
            state.has_trusted_servers()
        );
        Ok(Self {
            state,
            client: None,
            launcher,
            password_session: None,
            paranoia_mode: None,
            needs_sync: false,
        })
    }

    pub async fn reset_pairing(&mut self) -> Result<(), SessionError> {
        self.clear_connection();
        self.state.reset_pairing().await?;
        Ok(())
    }

    pub fn is_paired(&self) -> bool {
        self.state.has_trusted_servers()
    }

    /// Dropping this future drops the in-flight IPC connection, which lets the
    /// desktop host cancel its dependent native UI child.
    pub async fn request(
        &mut self,
        operation: Operation,
    ) -> Result<OperationSuccess, SessionRequestError> {
        crate::diagnostics::diagnostic!(
            "keeless-passkey-windows: session connecting to desktop host"
        );
        if let Err(error) = self.ensure_connected(true).await {
            crate::diagnostics::diagnostic!(
                "keeless-passkey-windows: session connection failed ({})",
                request_error_kind(&error)
            );
            return Err(error);
        }
        self.request_connected(operation).await
    }

    #[cfg(windows)]
    pub(crate) async fn request_with_sync(
        &mut self,
        operation: Operation,
        api: &crate::api::Api,
    ) -> Result<OperationSuccess, SessionRequestError> {
        self.ensure_connected(true).await?;
        if self.needs_sync {
            if let Err(error) = self.sync_credentials(api).await {
                if is_transport_error(&error) {
                    self.clear_connection();
                    self.ensure_connected(true).await?;
                    self.sync_credentials(api).await?;
                } else {
                    return Err(error);
                }
            }
        }
        self.request_connected(operation).await
    }

    async fn request_connected(
        &mut self,
        operation: Operation,
    ) -> Result<OperationSuccess, SessionRequestError> {
        let uses_secret = supports_password_session(&operation);
        if uses_secret && self.paranoia_mode == Some(true) && self.password_session.is_none() {
            self.ensure_password_session().await?;
        }
        let (operation, retry) = with_password_session(operation, self.password_session.clone());
        let result = self.request_once(operation).await;
        match result {
            Err(error) if uses_secret && has_operation_code(&error, "password_session_invalid") => {
                self.password_session = None;
                self.ensure_password_session().await?;
                let retry = retry.ok_or(error)?;
                self.request_once(retry.with_session(self.password_session.clone()))
                    .await
            }
            Err(error) if has_operation_code(&error, "database_locked") => {
                self.clear_connection();
                Err(error)
            }
            other => other,
        }
    }

    async fn request_once(
        &mut self,
        operation: Operation,
    ) -> Result<OperationSuccess, SessionRequestError> {
        crate::diagnostics::diagnostic!(
            "keeless-passkey-windows: session sending desktop operation"
        );
        let client = self
            .client
            .as_mut()
            .ok_or(SessionRequestError::Client(ClientError::Rejected))?;
        match client.request(operation).await {
            Ok(success) => {
                crate::diagnostics::diagnostic!(
                    "keeless-passkey-windows: session received desktop operation result"
                );
                Ok(success)
            }
            Err(error) => {
                crate::diagnostics::diagnostic!(
                    "keeless-passkey-windows: session desktop operation failed ({})",
                    client_error_kind(&error)
                );
                if !matches!(error, ClientError::Operation { .. }) {
                    self.clear_connection();
                }
                Err(error.into())
            }
        }
    }

    async fn ensure_password_session(&mut self) -> Result<(), SessionRequestError> {
        if self.password_session.is_some() {
            return Ok(());
        }
        let client = self
            .client
            .as_mut()
            .ok_or(SessionRequestError::Client(ClientError::Rejected))?;
        self.password_session = Some(create_password_session(client).await?);
        Ok(())
    }

    #[cfg(windows)]
    pub(crate) async fn sync_credentials(
        &mut self,
        api: &crate::api::Api,
    ) -> Result<(), SessionRequestError> {
        let password_session = self.password_session.clone();
        let result = match self
            .request(Operation::GetPasskeys(GetPasskeysArgs {
                password: None,
                password_session,
            }))
            .await
        {
            Ok(OperationSuccess::GetPasskeys(result)) => result,
            Ok(_) => return Err(ClientError::Rejected.into()),
            Err(SessionRequestError::Client(ClientError::Operation { code, .. }))
                if code == "database_locked" || code == "password_required" =>
            {
                if code == "database_locked" {
                    self.ensure_connected(true).await?;
                } else {
                    self.password_session = None;
                }
                self.ensure_password_session().await?;
                let password_session = self.password_session.clone();
                match self
                    .request_connected(Operation::GetPasskeys(GetPasskeysArgs {
                        password: None,
                        password_session,
                    }))
                    .await?
                {
                    OperationSuccess::GetPasskeys(result) => result,
                    _ => return Err(ClientError::Rejected.into()),
                }
            }
            Err(error) => return Err(error),
        };
        let desired = result
            .credentials
            .into_iter()
            .map(|credential| {
                Ok(crate::credential_cache::CredentialDetails {
                    credential_id: URL_SAFE_NO_PAD
                        .decode(credential.credential_id)
                        .map_err(|_| ClientError::Rejected)?,
                    rp_id: credential.rp_id,
                    rp_name: credential.rp_name,
                    user_id: URL_SAFE_NO_PAD
                        .decode(credential.user_id)
                        .map_err(|_| ClientError::Rejected)?,
                    user_name: credential.user_name,
                    user_display_name: credential.user_display_name,
                })
            })
            .collect::<Result<Vec<_>, ClientError>>()?;

        crate::diagnostics::diagnostic!(
            "keeless-passkey-windows: credential sync found {} credential",
            desired.len()
        );

        match crate::credential_cache::sync(api, &desired) {
            Ok(()) => {
                crate::diagnostics::diagnostic!(
                    "keeless-passkey-windows: credential cache synchronization completed",
                );
                self.needs_sync = false;
            }
            Err(error) => {
                crate::diagnostics::diagnostic!(
                    "keeless-passkey-windows: credential cache synchronization degraded ({error})"
                );
            }
        }

        Ok(())
    }

    /// Query a trusted running host without starting the desktop app or creating
    /// a first-use pairing prompt. Any unavailable or unexpected result is
    /// treated as locked by the COM boundary.
    pub async fn lock_status(&mut self) -> Result<DatabaseStatus, ClientError> {
        if self.state.trusted_server(UNTRUSTED_ENDPOINT_ID).is_none() {
            crate::diagnostics::diagnostic!(
                "keeless-passkey-windows: lock status unavailable because no trusted bootstrap endpoint exists"
            );
            return Err(ClientError::Rejected);
        }
        crate::diagnostics::diagnostic!(
            "keeless-passkey-windows: lock status connecting to trusted desktop host"
        );
        let mut client =
            match CoreClient::connect(&mut self.state, UNTRUSTED_ENDPOINT_ID, None).await {
                Ok(client) => client,
                Err(error) => {
                    crate::diagnostics::diagnostic!(
                        "keeless-passkey-windows: lock status connection failed ({})",
                        client_error_kind(&error)
                    );
                    return Err(error);
                }
            };
        crate::diagnostics::diagnostic!(
            "keeless-passkey-windows: lock status requesting desktop status"
        );
        let status = match client
            .request(Operation::GetCoreStatus(GetCoreStatusArgs {}))
            .await
        {
            Ok(OperationSuccess::GetCoreStatus(status)) => {
                crate::diagnostics::diagnostic!(
                    "keeless-passkey-windows: lock status received desktop status"
                );
                Ok(status.database)
            }
            Ok(_) => {
                crate::diagnostics::diagnostic!(
                    "keeless-passkey-windows: lock status received unexpected desktop result"
                );
                Err(ClientError::Rejected)
            }
            Err(error) => {
                crate::diagnostics::diagnostic!(
                    "keeless-passkey-windows: lock status request failed ({})",
                    client_error_kind(&error)
                );
                Err(error)
            }
        };
        if matches!(status, Ok(DatabaseStatus::Locked)) {
            self.clear_connection();
        }
        status
    }

    async fn ensure_connected(&mut self, launch_desktop: bool) -> Result<(), SessionRequestError> {
        if self.client.is_none() {
            if launch_desktop {
                if let Some(launcher) = &self.launcher {
                    crate::diagnostics::diagnostic!(
                        "keeless-passkey-windows: starting desktop host if needed"
                    );
                    if let Err(error) = launcher.ensure_running().await {
                        crate::diagnostics::diagnostic!(
                            "keeless-passkey-windows: desktop host did not start"
                        );
                        return Err(ClientError::from(error).into());
                    }
                    crate::diagnostics::diagnostic!(
                        "keeless-passkey-windows: desktop host is reachable"
                    );
                } else {
                    crate::diagnostics::diagnostic!(
                        "keeless-passkey-windows: no desktop launcher is configured"
                    );
                }
            }
            crate::diagnostics::diagnostic!(
                "keeless-passkey-windows: connecting to bootstrap endpoint"
            );
            let mut untrusted =
                match CoreClient::connect(&mut self.state, UNTRUSTED_ENDPOINT_ID, None).await {
                    Ok(client) => client,
                    Err(error) => {
                        crate::diagnostics::diagnostic!(
                            "keeless-passkey-windows: bootstrap connection failed ({})",
                            client_error_kind(&error)
                        );
                        return Err(error.into());
                    }
                };
            let OperationSuccess::GetCoreStatus(status) = untrusted
                .request(Operation::GetCoreStatus(GetCoreStatusArgs {}))
                .await?
            else {
                return Err(ClientError::Rejected.into());
            };
            match status.database {
                DatabaseStatus::Unlocked => {}
                DatabaseStatus::Locked => {
                    let password_session = create_password_session(&mut untrusted).await?;
                    self.password_session = Some(password_session.clone());
                    let OperationSuccess::Unlock(_) = untrusted
                        .request(Operation::Unlock(UnlockArgs {
                            password: None,
                            password_session: Some(password_session),
                        }))
                        .await?
                    else {
                        return Err(ClientError::Rejected.into());
                    };
                }
                DatabaseStatus::NotExist => {
                    if let Some(launcher) = &self.launcher {
                        let _ = launcher.show();
                    }
                    return Err(SessionRequestError::DatabaseUnavailable);
                }
            }
            crate::diagnostics::diagnostic!(
                "keeless-passkey-windows: requesting desktop endpoint upgrade"
            );
            let upgrade = match untrusted.request(Operation::Upgrade(UpgradeArgs {})).await {
                Ok(OperationSuccess::Upgrade(upgrade)) => upgrade,
                Ok(_) => {
                    crate::diagnostics::diagnostic!(
                        "keeless-passkey-windows: desktop returned unexpected upgrade result"
                    );
                    return Err(ClientError::Rejected.into());
                }
                Err(error) => {
                    crate::diagnostics::diagnostic!(
                        "keeless-passkey-windows: desktop upgrade failed ({})",
                        client_error_kind(&error)
                    );
                    return Err(error.into());
                }
            };
            crate::diagnostics::diagnostic!(
                "keeless-passkey-windows: connecting to trusted core endpoint"
            );
            let client = match CoreClient::connect(
                &mut self.state,
                CORE_ENDPOINT_ID,
                Some(upgrade.public_key),
            )
            .await
            {
                Ok(client) => client,
                Err(error) => {
                    crate::diagnostics::diagnostic!(
                        "keeless-passkey-windows: core connection failed ({})",
                        client_error_kind(&error)
                    );
                    return Err(error.into());
                }
            };
            self.client = Some(client);
            self.needs_sync = true;
            self.refresh_mode().await?;
            crate::diagnostics::diagnostic!(
                "keeless-passkey-windows: connected to trusted core endpoint"
            );
        }
        Ok(())
    }

    async fn refresh_mode(&mut self) -> Result<(), SessionRequestError> {
        let client = self
            .client
            .as_mut()
            .ok_or(SessionRequestError::Client(ClientError::Rejected))?;
        let config = match client
            .request(Operation::GetConfig(GetConfigArgs {}))
            .await?
        {
            OperationSuccess::GetConfig(result) => result.config,
            _ => return Err(ClientError::Rejected.into()),
        };
        self.paranoia_mode = Some(config.paranoia_mode);
        if config.paranoia_mode {
            self.ensure_password_session().await?;
        }
        Ok(())
    }

    fn clear_connection(&mut self) {
        self.client = None;
        self.password_session = None;
        self.paranoia_mode = None;
        self.needs_sync = true;
    }
}

async fn create_password_session(client: &mut CoreClient) -> Result<String, SessionRequestError> {
    match client
        .request(Operation::CreatePasswordSession(
            CreatePasswordSessionArgs { password: None },
        ))
        .await?
    {
        OperationSuccess::CreatePasswordSession(result) => Ok(result.password_session),
        _ => Err(ClientError::Rejected.into()),
    }
}

fn supports_password_session(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::GetPasskeys(_) | Operation::RegisterPasskey(_) | Operation::AssertPasskey(_)
    )
}

fn has_operation_code(error: &SessionRequestError, expected: &str) -> bool {
    matches!(
        error,
        SessionRequestError::Client(ClientError::Operation { code, .. }) if code == expected
    )
}

#[cfg(windows)]
fn is_transport_error(error: &SessionRequestError) -> bool {
    matches!(
        error,
        SessionRequestError::Client(error) if !matches!(error, ClientError::Operation { .. })
    )
}

enum PasswordSessionOperation {
    GetPasskeys(GetPasskeysArgs),
    RegisterPasskey(RegisterPasskeyArgs),
    AssertPasskey(AssertPasskeyArgs),
}

impl PasswordSessionOperation {
    fn with_session(self, password_session: Option<String>) -> Operation {
        match self {
            Self::GetPasskeys(mut args) => {
                args.password_session = password_session;
                Operation::GetPasskeys(args)
            }
            Self::RegisterPasskey(mut args) => {
                args.password_session = password_session;
                Operation::RegisterPasskey(args)
            }
            Self::AssertPasskey(mut args) => {
                args.password_session = password_session;
                Operation::AssertPasskey(args)
            }
        }
    }
}

fn with_password_session(
    operation: Operation,
    password_session: Option<String>,
) -> (Operation, Option<PasswordSessionOperation>) {
    match operation {
        Operation::GetPasskeys(mut args) => {
            let retry = PasswordSessionOperation::GetPasskeys(args.clone());
            args.password_session = password_session;
            (Operation::GetPasskeys(args), Some(retry))
        }
        Operation::RegisterPasskey(mut args) => {
            let retry = PasswordSessionOperation::RegisterPasskey(args.clone());
            args.password_session = password_session;
            (Operation::RegisterPasskey(args), Some(retry))
        }
        Operation::AssertPasskey(mut args) => {
            let retry = PasswordSessionOperation::AssertPasskey(args.clone());
            args.password_session = password_session;
            (Operation::AssertPasskey(args), Some(retry))
        }
        operation => (operation, None),
    }
}

fn client_error_kind(error: &ClientError) -> String {
    match error {
        ClientError::Ipc(_) => "ipc".into(),
        ClientError::Wire(_) => "lesswire".into(),
        ClientError::State(_) => "state".into(),
        ClientError::Launcher(_) => "launcher".into(),
        ClientError::Malformed(_) => "malformed-response".into(),
        ClientError::Rejected => "rejected".into(),
        ClientError::MismatchedResponse => "mismatched-response".into(),
        ClientError::ServerIdentityChanged => "server-identity-changed".into(),
        ClientError::Operation { code, .. } => format!("core-operation:{code}"),
    }
}

fn request_error_kind(error: &SessionRequestError) -> String {
    match error {
        SessionRequestError::Client(error) => client_error_kind(error),
        SessionRequestError::DatabaseUnavailable => "database-unavailable".into(),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionRequestError {
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error("selected database is unavailable")]
    DatabaseUnavailable,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("cannot locate the Keeless data directory: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    State(#[from] keeless_host_desktop_shared::state::StateError),
}
