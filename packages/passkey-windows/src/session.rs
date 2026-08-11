//! Authenticated local IPC state for the Windows passkey provider.

use keeless_host_desktop_shared::state::{ClientState, FileStore};
use keeless_host_desktop_shared::{ClientError, CoreClient, DesktopLauncher};
use keeless_schema::{Operation, OperationSuccess};

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
            state: ClientState::load(store).await?,
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
        self.state.trusted_server().is_some()
    }

    /// Dropping this future drops the in-flight IPC connection, which lets the
    /// desktop host cancel its dependent native UI child.
    pub async fn request(&mut self, operation: Operation) -> Result<OperationSuccess, ClientError> {
        if self.client.is_none() {
            if let Some(launcher) = &self.launcher {
                launcher.ensure_running().await?;
            }
            self.client = Some(CoreClient::connect(&mut self.state).await?);
        }
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
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("cannot locate the Keeless data directory: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    State(#[from] keeless_host_desktop_shared::state::StateError),
}
