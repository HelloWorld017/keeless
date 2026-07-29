//! Starting the desktop app when a sidecar needs its local IPC host.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use thiserror::Error;

use crate::ipc;

const START_TIMEOUT: Duration = Duration::from_secs(10);
const RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// Starts a desktop executable without showing its main window.
///
/// Sidecars receive this path from their installation or command-line
/// configuration. It is deliberately not resolved through `PATH`: a process
/// that can edit `PATH` must not be able to choose the desktop process that
/// owns the user's database.
#[derive(Clone, Debug)]
pub struct DesktopLauncher {
    executable: PathBuf,
}

impl DesktopLauncher {
    /// Bind the launcher to an installed desktop executable.
    pub fn new(executable: PathBuf) -> Result<Self, LauncherError> {
        if !executable.is_absolute() {
            return Err(LauncherError::InvalidPath(executable));
        }
        if !std::fs::metadata(&executable)?.is_file() {
            return Err(LauncherError::InvalidPath(executable));
        }
        Ok(Self { executable })
    }

    /// Start the desktop app if its current-user IPC endpoint is unavailable.
    pub async fn ensure_running(&self) -> Result<(), LauncherError> {
        if ipc::Client::ping().await.is_ok() {
            return Ok(());
        }

        let mut child = desktop_command(&self.executable)
            .spawn()
            .map_err(|source| LauncherError::Spawn {
                executable: self.executable.clone(),
                source,
            })?;
        // The sidecar outlives the desktop app, so reap the child when it exits
        // instead of retaining a zombie for the daemon's lifetime.
        std::thread::spawn(move || {
            let _ = child.wait();
        });

        let deadline = tokio::time::Instant::now() + START_TIMEOUT;
        loop {
            if ipc::Client::ping().await.is_ok() {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(LauncherError::TimedOut);
            }
            tokio::time::sleep(RETRY_INTERVAL).await;
        }
    }
}

fn desktop_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .arg("--minimized")
        // The daemon must not keep the desktop process attached to its stdio.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[derive(Debug, Error)]
pub enum LauncherError {
    #[error("desktop executable must be an absolute regular file: {0:?}")]
    InvalidPath(PathBuf),
    #[error("cannot inspect desktop executable: {0}")]
    Io(#[from] std::io::Error),
    #[error("cannot start desktop executable {executable:?}: {source}")]
    Spawn {
        executable: PathBuf,
        source: std::io::Error,
    },
    #[error("desktop app did not open its IPC endpoint within {START_TIMEOUT:?}")]
    TimedOut,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_executables() {
        let error = DesktopLauncher::new(PathBuf::from("keeless")).unwrap_err();
        assert!(matches!(error, LauncherError::InvalidPath(path) if path == Path::new("keeless")));
    }

    #[test]
    fn starts_the_configured_program_minimized() {
        let executable = Path::new("/opt/keeless/keeless");
        let command = desktop_command(executable);
        assert_eq!(command.get_program(), executable.as_os_str());
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["--minimized"]);
    }
}
