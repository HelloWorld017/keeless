//! Virtual FIDO2 authenticator serving Keeless passkeys.
//!
//! The daemon presents a virtual HID device to the system, so browsers treat
//! Keeless as an ordinary security key. Requests arriving over CTAPHID become
//! Keeless operations sent to the running desktop app, which owns the database.
//!
//! User presence is collected here rather than by the app: only the process that
//! owns the prompt can take it down when the browser cancels, and the app's own
//! request lock would otherwise stall its whole UI for the length of a ceremony.

pub mod authenticator;
pub mod consent;
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

use std::ffi::OsString;
use std::path::PathBuf;

use keeless_host_desktop_shared::DesktopLauncher;
use thiserror::Error;

/// The dialog helper's file name, looked for beside this executable.
const NATIVE_UI_NAME: &str = "keeless-native-ui";

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
usage: keeless-passkey-linux [run] [--native-ui <path>] [--desktop <path>]
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
    /// Only the daemon spawns dialogs, and only Linux has a daemon.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    native_ui: PathBuf,
    /// Optional because direct users may prefer requests to fail while the app
    /// is closed rather than permit this daemon to launch it.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    desktop: Option<DesktopLauncher>,
}

impl Options {
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let mut native_ui = None;
        let mut desktop = None;
        let mut args = args.into_iter();
        while let Some(argument) = args.next() {
            match argument.to_str() {
                Some("--native-ui") => {
                    native_ui =
                        Some(PathBuf::from(args.next().ok_or_else(|| {
                            VhidError::Usage("--native-ui requires a path".into())
                        })?));
                }
                Some("--desktop") => {
                    let path = PathBuf::from(
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
        Ok(Self {
            native_ui: native_ui.map(Ok).unwrap_or_else(default_native_ui)?,
            desktop,
        })
    }
}

/// The dialog helper installed beside this executable.
///
/// Resolved from the daemon's own location rather than looked up on `PATH`: the
/// helper is the only thing standing between a web page and a signature, and a
/// `PATH` entry the user can write to is a place anyone can put a program that
/// approves everything.
fn default_native_ui() -> Result<PathBuf> {
    let executable = std::env::current_exe().map_err(VhidError::Device)?;
    let directory = executable.parent().ok_or_else(|| {
        VhidError::Usage("cannot locate the directory holding keeless-passkey-linux".into())
    })?;
    Ok(directory.join(NATIVE_UI_NAME))
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
            let consent =
                consent::ConsentPrompt::new(options.native_ui).map_err(VhidError::Device)?;
            let authenticator = authenticator::Authenticator::new(session, consent);
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
    fn looks_for_the_dialog_helper_beside_this_executable() {
        let default = options(&[]).unwrap().native_ui;
        assert!(default.is_absolute(), "a bare name would be found on PATH");
        assert_eq!(default.file_name().unwrap(), NATIVE_UI_NAME);
        assert_eq!(
            default.parent().unwrap(),
            std::env::current_exe().unwrap().parent().unwrap()
        );

        assert_eq!(
            options(&["--native-ui", "/opt/keeless/ui"])
                .unwrap()
                .native_ui,
            PathBuf::from("/opt/keeless/ui")
        );
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
        assert!(matches!(
            options(&["--native-ui"]),
            Err(VhidError::Usage(_))
        ));
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
