//! Authenticated local IPC state for the Windows passkey provider.

use keeless_host_desktop_shared::client::{CORE_ENDPOINT_ID, UNTRUSTED_ENDPOINT_ID};
use keeless_host_desktop_shared::state::{ClientState, FileStore};
use keeless_host_desktop_shared::{ClientError, CoreClient, DesktopLauncher};
use keeless_lesswire::KeyScope;
use keeless_schema::{DatabaseStatus, GetCoreStatusArgs, Operation, OperationSuccess, UpgradeArgs};

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
        Ok(Self {
            state: ClientState::load(store, KeyScope::Passkey).await?,
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
    pub async fn request(&mut self, operation: Operation) -> Result<OperationSuccess, ClientError> {
        self.ensure_connected(true).await?;
        let client = self.client.as_mut().expect("client was just connected");
        match client.request(operation).await {
            Ok(success) => Ok(success),
            Err(error) => {
                if !matches!(error, ClientError::Operation { .. }) {
                    self.client = None;
                }
                Err(error)
            }
        }
    }

    /// Query a trusted running host without starting the desktop app or creating
    /// a first-use pairing prompt. Any unavailable or unexpected result is
    /// treated as locked by the COM boundary.
    pub async fn lock_status(&mut self) -> Result<DatabaseStatus, ClientError> {
        if self.state.trusted_server(UNTRUSTED_ENDPOINT_ID).is_none() {
            return Err(ClientError::Rejected);
        }
        let mut client = CoreClient::connect(&mut self.state, UNTRUSTED_ENDPOINT_ID, None).await?;
        match client
            .request(Operation::GetCoreStatus(GetCoreStatusArgs {}))
            .await
        {
            Ok(OperationSuccess::GetCoreStatus(status)) => Ok(status.database),
            Ok(_) => Err(ClientError::Rejected),
            Err(error) => Err(error),
        }
    }

    async fn ensure_connected(&mut self, launch_desktop: bool) -> Result<(), ClientError> {
        if self.client.is_none() {
            if launch_desktop {
                if let Some(launcher) = &self.launcher {
                    launcher.ensure_running().await?;
                }
            }
            let mut untrusted =
                CoreClient::connect(&mut self.state, UNTRUSTED_ENDPOINT_ID, None).await?;
            let OperationSuccess::Upgrade(upgrade) = untrusted
                .request(Operation::Upgrade(UpgradeArgs {}))
                .await?
            else {
                return Err(ClientError::Rejected);
            };
            self.client = Some(
                CoreClient::connect(&mut self.state, CORE_ENDPOINT_ID, Some(upgrade.public_key))
                    .await?,
            );
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("cannot locate the Keeless data directory: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    State(#[from] keeless_host_desktop_shared::state::StateError),
}
