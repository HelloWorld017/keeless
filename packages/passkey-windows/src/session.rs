//! Authenticated local IPC state for the Windows passkey provider.

use keeless_host_desktop_shared::client::{CORE_ENDPOINT_ID, UNTRUSTED_ENDPOINT_ID};
use keeless_host_desktop_shared::state::{ClientState, FileStore};
use keeless_host_desktop_shared::{ClientError, CoreClient, DesktopLauncher};
use keeless_lesswire::KeyScope;
use keeless_schema::{
    DatabaseStatus, GetCoreStatusArgs, Operation, OperationSuccess, UnlockArgs, UpgradeArgs,
};

/// File holding the provider's lesswire identity and pinned desktop host key.
pub const STATE_FILE: &str = "passkey-windows-state.json";

/// A lazily connected Core client, recreated when the desktop host restarts.
pub struct Session {
    state: ClientState,
    client: Option<CoreClient>,
    launcher: Option<DesktopLauncher>,
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
        })
    }

    pub async fn reset_pairing(&mut self) -> Result<(), SessionError> {
        self.client = None;
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
        crate::diagnostics::diagnostic!(
            "keeless-passkey-windows: session sending desktop operation"
        );
        let client = self.client.as_mut().expect("client was just connected");
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
                    self.client = None;
                }
                Err(error.into())
            }
        }
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
        match client
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
        }
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
                    let OperationSuccess::Unlock(_) = untrusted
                        .request(Operation::Unlock(UnlockArgs {
                            password: None,
                            password_session: None,
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
            crate::diagnostics::diagnostic!(
                "keeless-passkey-windows: connected to trusted core endpoint"
            );
        }
        Ok(())
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
