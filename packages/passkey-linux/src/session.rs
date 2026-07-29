//! Talking to the running Keeless desktop app.

use keeless_host_desktop_shared::state::{ClientState, FileStore};
use keeless_host_desktop_shared::{ClientError, CoreClient, DesktopLauncher};
use keeless_schema::{Operation, OperationSuccess};

/// File holding this daemon's wire identity, next to the app's own state.
pub const STATE_FILE: &str = "vhid-state.json";

/// A connection to the app, reconnected whenever the app restarts.
///
/// The connection is established lazily so the daemon can hold its virtual HID
/// device open while the app is closed. A configured launcher starts the app
/// before the first connection attempt; without one, the request fails without
/// taking the device down.
pub struct Session {
    state: ClientState,
    client: Option<CoreClient>,
    launcher: Option<DesktopLauncher>,
}

impl Session {
    pub async fn load() -> Result<Self, SessionError> {
        Self::load_with_launcher(None).await
    }

    /// Load the client state and optionally arrange to start the desktop app.
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

    /// Forget the pinned host key so the next request pairs with the app again.
    pub async fn reset_pairing(&mut self) -> Result<(), SessionError> {
        self.client = None;
        self.state.reset_pairing().await?;
        Ok(())
    }

    pub fn is_paired(&self) -> bool {
        self.state.trusted_server().is_some()
    }

    /// Run an operation, connecting or reconnecting as needed.
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
                // Anything but a refused operation means the connection is gone
                // or the app no longer trusts us; the next request starts over.
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
