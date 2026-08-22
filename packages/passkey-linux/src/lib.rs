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
pub mod doctor;
#[cfg(target_os = "linux")]
pub mod instance;
#[cfg(target_os = "linux")]
pub mod service;
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
    #[error("passkey service failed: {0}")]
    Service(String),
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
       keeless-passkey-linux --enable [--desktop <path>]
       keeless-passkey-linux --disable
       keeless-passkey-linux doctor [--json]
       keeless-passkey-linux reset-pairing

  run            serve passkeys over a virtual HID device (default)
  --desktop      absolute Keeless desktop executable to start when needed
  --enable       install and start the system passkey service
  --disable      stop and disable the system passkey service
  doctor         report service and virtual FIDO device status
  reset-pairing  forget the desktop app, so the next run asks for approval again";

pub fn main(args: impl IntoIterator<Item = OsString>) -> Result<()> {
    let Some(command) = Command::parse(args)? else {
        println!("{USAGE}");
        return Ok(());
    };

    match command {
        Command::Run(options) => run_command(options),
        Command::Enable(options) => enable_command(options),
        Command::Disable => disable_command(),
        Command::Doctor(options) => doctor_command(options),
        Command::ResetPairing => block_on(reset_pairing()),
        Command::InternalInstallService { user_id, desktop } => {
            internal_install_service(user_id, desktop)
        }
        Command::InternalDisableService { user_id } => internal_disable_service(user_id),
    }
}

enum Command {
    Run(RunOptions),
    Enable(EnableOptions),
    Disable,
    Doctor(DoctorOptions),
    ResetPairing,
    /// Only `pkexec` should invoke these commands. They keep the public command
    /// line limited to enable/disable while making the privileged phase explicit.
    InternalInstallService {
        user_id: u32,
        desktop: Option<std::path::PathBuf>,
    },
    InternalDisableService {
        user_id: u32,
    },
}

impl Command {
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Self>> {
        let mut args = args.into_iter();
        let Some(first) = args.next() else {
            return Ok(Some(Self::Run(RunOptions { desktop: None })));
        };
        match first.to_str() {
            Some("--help" | "-h") => Ok(None),
            Some("run") => Ok(Some(Self::Run(RunOptions::parse(args)?))),
            Some("--enable") => Ok(Some(Self::Enable(EnableOptions::parse(args)?))),
            Some("--disable") => {
                reject_remaining(args)?;
                Ok(Some(Self::Disable))
            }
            Some("doctor") => Ok(Some(Self::Doctor(DoctorOptions::parse(args)?))),
            Some("reset-pairing") => {
                reject_remaining(args)?;
                Ok(Some(Self::ResetPairing))
            }
            Some("--internal-install-service") => {
                let (user_id, desktop) = InternalOptions::parse(args, true)?;
                Ok(Some(Self::InternalInstallService { user_id, desktop }))
            }
            Some("--internal-disable-service") => {
                let (user_id, desktop) = InternalOptions::parse(args, false)?;
                if desktop.is_some() {
                    return Err(VhidError::Usage(
                        "--internal-disable-service does not accept --desktop".into(),
                    ));
                }
                Ok(Some(Self::InternalDisableService { user_id }))
            }
            _ => {
                let mut run_args = vec![first];
                run_args.extend(args);
                Ok(Some(Self::Run(RunOptions::parse(run_args)?)))
            }
        }
    }
}

struct RunOptions {
    /// Optional because direct users may prefer requests to fail while the app
    /// is closed rather than permit this daemon to launch it.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    desktop: Option<DesktopLauncher>,
}

impl RunOptions {
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let desktop = parse_desktop(args)?.map(DesktopLauncher::new).transpose()?;
        Ok(Self { desktop })
    }
}

struct EnableOptions {
    desktop: Option<std::path::PathBuf>,
}

impl EnableOptions {
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self> {
        Ok(Self {
            desktop: parse_desktop(args)?,
        })
    }
}

struct DoctorOptions {
    json: bool,
}

impl DoctorOptions {
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let mut json = false;
        for argument in args {
            match argument.to_str() {
                Some("--json") if !json => json = true,
                _ => {
                    return Err(VhidError::Usage(format!(
                        "unexpected argument: {}\n\n{USAGE}",
                        argument.to_string_lossy()
                    )));
                }
            }
        }
        Ok(Self { json })
    }
}

struct InternalOptions;

impl InternalOptions {
    fn parse(
        args: impl IntoIterator<Item = OsString>,
        allow_desktop: bool,
    ) -> Result<(u32, Option<std::path::PathBuf>)> {
        let mut user_id = None;
        let mut desktop = None;
        let mut args = args.into_iter();
        while let Some(argument) = args.next() {
            match argument.to_str() {
                Some("--uid") if user_id.is_none() => {
                    let value = args.next().ok_or_else(|| {
                        VhidError::Usage("--uid requires a numeric user ID".into())
                    })?;
                    user_id = Some(value.to_string_lossy().parse::<u32>().map_err(|_| {
                        VhidError::Usage("--uid requires a numeric user ID".into())
                    })?);
                }
                Some("--desktop") if allow_desktop && desktop.is_none() => {
                    let path = args
                        .next()
                        .ok_or_else(|| VhidError::Usage("--desktop requires a path".into()))?;
                    desktop = Some(validate_desktop_path(std::path::PathBuf::from(path))?);
                }
                _ => {
                    return Err(VhidError::Usage(format!(
                        "unexpected argument: {}\n\n{USAGE}",
                        argument.to_string_lossy()
                    )));
                }
            }
        }
        Ok((
            user_id.ok_or_else(|| VhidError::Usage("--uid is required".into()))?,
            desktop,
        ))
    }
}

fn parse_desktop(args: impl IntoIterator<Item = OsString>) -> Result<Option<std::path::PathBuf>> {
    let mut desktop = None;
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        match argument.to_str() {
            Some("--desktop") if desktop.is_none() => {
                let path = args
                    .next()
                    .ok_or_else(|| VhidError::Usage("--desktop requires a path".into()))?;
                desktop = Some(validate_desktop_path(std::path::PathBuf::from(path))?);
            }
            _ => {
                return Err(VhidError::Usage(format!(
                    "unexpected argument: {}\n\n{USAGE}",
                    argument.to_string_lossy()
                )));
            }
        }
    }
    Ok(desktop)
}

fn validate_desktop_path(path: std::path::PathBuf) -> Result<std::path::PathBuf> {
    DesktopLauncher::new(path.clone())?;
    Ok(path)
}

fn reject_remaining(args: impl IntoIterator<Item = OsString>) -> Result<()> {
    if let Some(argument) = args.into_iter().next() {
        return Err(VhidError::Usage(format!(
            "unexpected argument: {}\n\n{USAGE}",
            argument.to_string_lossy()
        )));
    }
    Ok(())
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
fn enable_command(options: EnableOptions) -> Result<()> {
    service::enable(options.desktop).map_err(VhidError::Service)
}

#[cfg(target_os = "linux")]
fn disable_command() -> Result<()> {
    service::disable().map_err(VhidError::Service)
}

#[cfg(target_os = "linux")]
fn internal_install_service(user_id: u32, desktop: Option<std::path::PathBuf>) -> Result<()> {
    service::install_for_user(user_id, desktop).map_err(VhidError::Service)
}

#[cfg(target_os = "linux")]
fn internal_disable_service(user_id: u32) -> Result<()> {
    service::disable_for_user(user_id).map_err(VhidError::Service)
}

#[cfg(target_os = "linux")]
fn doctor_command(options: DoctorOptions) -> Result<()> {
    let state = doctor::diagnose(service::current_user_id());
    if options.json {
        println!(
            "{}",
            serde_json::to_string(&state).map_err(|error| VhidError::Service(format!(
                "cannot encode doctor output: {error}"
            )))?
        );
    } else {
        doctor::print_human(&state);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_command(options: RunOptions) -> Result<()> {
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
fn enable_command(_options: EnableOptions) -> Result<()> {
    Err(VhidError::Unsupported)
}

#[cfg(not(target_os = "linux"))]
fn disable_command() -> Result<()> {
    Err(VhidError::Unsupported)
}

#[cfg(not(target_os = "linux"))]
fn internal_install_service(_user_id: u32, _desktop: Option<std::path::PathBuf>) -> Result<()> {
    Err(VhidError::Unsupported)
}

#[cfg(not(target_os = "linux"))]
fn internal_disable_service(_user_id: u32) -> Result<()> {
    Err(VhidError::Unsupported)
}

#[cfg(not(target_os = "linux"))]
fn doctor_command(_options: DoctorOptions) -> Result<()> {
    Err(VhidError::Unsupported)
}

#[cfg(not(target_os = "linux"))]
fn run_command(_options: RunOptions) -> Result<()> {
    Err(VhidError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(args: &[&str]) -> Result<RunOptions> {
        RunOptions::parse(args.iter().map(OsString::from))
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
    fn parses_service_commands_without_the_removed_setup_command() {
        assert!(matches!(
            Command::parse(["--enable"].map(OsString::from)),
            Ok(Some(Command::Enable(_)))
        ));
        assert!(matches!(
            Command::parse(["--disable"].map(OsString::from)),
            Ok(Some(Command::Disable))
        ));
        assert!(matches!(
            Command::parse(["doctor", "--json"].map(OsString::from)),
            Ok(Some(Command::Doctor(DoctorOptions { json: true })))
        ));
        assert!(matches!(
            Command::parse(["setup"].map(OsString::from)),
            Err(VhidError::Usage(_))
        ));
    }

    #[test]
    fn exit_codes_separate_misuse_from_a_missing_device() {
        assert_eq!(VhidError::Usage(String::new()).exit_code(), 2);
        assert_eq!(VhidError::Device(std::io::Error::other("x")).exit_code(), 3);
    }
}
