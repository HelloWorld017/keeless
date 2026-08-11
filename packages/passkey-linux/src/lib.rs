//! Virtual FIDO2 authenticator serving Keeless passkeys.
//!
//! The daemon presents a virtual HID device to the system, so browsers treat
//! Keeless as an ordinary security key. Requests arriving over CTAPHID become
//! Keeless operations sent to the running desktop app, which owns the database.
//!
//! CTAP parsing, framing, keepalive, and cancellation live here. Core owns every
//! ceremony decision and asks the desktop host to present user consent.

pub mod authenticator;
pub mod ctaphid;
pub mod session;

#[cfg(target_os = "linux")]
pub mod daemon;
#[cfg(target_os = "linux")]
pub mod instance;
#[cfg(target_os = "linux")]
pub mod setup;
#[cfg(target_os = "linux")]
pub mod uhid;

use keeless_host_desktop_shared::DesktopLauncher;
use std::ffi::OsString;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VhidError {
    #[error("{0}")]
    Usage(String),
    #[error(transparent)]
    Session(#[from] session::SessionError),
    #[error("virtual HID device failed: {0}")]
    Device(std::io::Error),
    #[error(transparent)]
    Desktop(#[from] keeless_host_desktop_shared::LauncherError),
    #[error("this platform has no virtual HID support")]
    Unsupported,
}

impl VhidError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Device(_) => 3,
            _ => 1,
        }
    }
}

pub type Result<T> = std::result::Result<T, VhidError>;

const USAGE: &str = "\
usage: keeless-passkey-linux [run] [--desktop <path>]
       keeless-passkey-linux setup
       keeless-passkey-linux doctor
       keeless-passkey-linux reset-pairing

  run            serve passkeys over a virtual HID device (default)
  --desktop      absolute Keeless desktop executable to start when needed
  setup          print the commands that grant access to /dev/uhid
  doctor         report whether the device and the desktop app are reachable
  reset-pairing  forget the desktop app, so the next run asks for approval again";

pub fn main(args: impl IntoIterator<Item = OsString>) -> Result<()> {
    let mut args = args.into_iter().peekable();
    let command = match args.peek().and_then(|value| value.to_str()) {
        Some("setup" | "doctor" | "reset-pairing" | "run") => args
            .next()
            .expect("peeked a command")
            .into_string()
            .expect("checked as UTF-8"),
        Some("--help" | "-h") => {
            println!("{USAGE}");
            return Ok(());
        }
        _ => "run".to_string(),
    };
    let options = Options::parse(args)?;

    match command.as_str() {
        "setup" => setup_command(),
        "doctor" => doctor_command(),
        "reset-pairing" => block_on(reset_pairing()),
        _ => run_command(options),
    }
}

struct Options {
    /// Optional because direct users may prefer requests to fail while the app
    /// is closed rather than permit this daemon to launch it.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    desktop: Option<DesktopLauncher>,
}

impl Options {
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let mut desktop = None;
        let mut args = args.into_iter();
        while let Some(argument) = args.next() {
            match argument.to_str() {
                Some("--desktop") => {
                    let path = std::path::PathBuf::from(
                        args.next()
                            .ok_or_else(|| VhidError::Usage("--desktop requires a path".into()))?,
                    );
                    desktop = Some(DesktopLauncher::new(path)?);
                }
                _ => {
                    return Err(VhidError::Usage(format!(
                        "unexpected argument: {}\n\n{USAGE}",
                        argument.to_string_lossy()
                    )));
                }
            }
        }
        Ok(Self { desktop })
    }
}

fn block_on<T>(future: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(VhidError::Device)?
        .block_on(future)
}

async fn reset_pairing() -> Result<()> {
    let mut session = session::Session::load().await?;
    session.reset_pairing().await?;
    println!("Forgot the paired desktop app; the next request will ask for approval.");
    Ok(())
}

#[cfg(target_os = "linux")]
fn setup_command() -> Result<()> {
    if setup::readiness() == setup::Readiness::Ready {
        println!("/dev/uhid is already accessible; no setup needed.");
        return Ok(());
    }
    setup::print_instructions();
    Ok(())
}

#[cfg(target_os = "linux")]
fn doctor_command() -> Result<()> {
    let readiness = setup::readiness();
    println!("device:  {}", readiness.describe());

    block_on(async {
        let session = session::Session::load().await?;
        println!(
            "pairing: {}",
            if session.is_paired() {
                "paired with the desktop app"
            } else {
                "not paired yet; the first request will ask for approval"
            }
        );
        match keeless_host_desktop_shared::CoreClient::ping().await {
            Ok(()) => println!("app:     reachable"),
            Err(error) => println!("app:     unreachable ({error})"),
        }
        Ok(())
    })?;

    if readiness != setup::Readiness::Ready {
        println!("\nRun `keeless-passkey-linux setup` for the commands that fix the device.");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_command(options: Options) -> Result<()> {
    let Some(_lock) = instance::InstanceLock::acquire().map_err(VhidError::Device)? else {
        eprintln!("keeless-passkey-linux: another instance is already running");
        return Ok(());
    };

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
        .map_err(VhidError::Device)?
        .block_on(async move {
            let session = session::Session::load_with_launcher(options.desktop).await?;
            let authenticator = authenticator::Authenticator::new(session);
            daemon::run(authenticator, shutdown_signal())
                .await
                .map_err(VhidError::Device)
        })
}

/// Resolves when the daemon should stop, on a termination or interrupt signal.
#[cfg(target_os = "linux")]
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let (Ok(mut terminate), Ok(mut interrupt)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = terminate.recv() => {}
        _ = interrupt.recv() => {}
    }
}

#[cfg(not(target_os = "linux"))]
fn setup_command() -> Result<()> {
    Err(VhidError::Unsupported)
}

#[cfg(not(target_os = "linux"))]
fn doctor_command() -> Result<()> {
    Err(VhidError::Unsupported)
}

#[cfg(not(target_os = "linux"))]
fn run_command(_options: Options) -> Result<()> {
    Err(VhidError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(args: &[&str]) -> Result<Options> {
        Options::parse(args.iter().map(OsString::from))
    }

    #[test]
    fn accepts_an_absolute_desktop_executable() {
        let executable = std::env::current_exe().unwrap();
        assert!(
            options(&["--desktop", executable.to_str().unwrap()])
                .unwrap()
                .desktop
                .is_some()
        );
        assert!(matches!(
            options(&["--desktop", "keeless"]),
            Err(VhidError::Desktop(
                keeless_host_desktop_shared::LauncherError::InvalidPath(_)
            ))
        ));
    }

    #[test]
    fn rejects_unknown_and_incomplete_arguments() {
        assert!(matches!(options(&["--desktop"]), Err(VhidError::Usage(_))));
        assert!(matches!(options(&["--nope"]), Err(VhidError::Usage(_))));
        assert!(matches!(options(&["stray"]), Err(VhidError::Usage(_))));
    }

    #[test]
    fn exit_codes_separate_misuse_from_a_missing_device() {
        assert_eq!(VhidError::Usage(String::new()).exit_code(), 2);
        assert_eq!(VhidError::Device(std::io::Error::other("x")).exit_code(), 3);
    }
}
