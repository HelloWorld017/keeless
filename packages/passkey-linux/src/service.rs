//! Privileged installation of the system service that owns the UHID descriptor.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

pub const MODULE_CONFIG_PATH: &str = "/etc/modules-load.d/keeless-uhid.conf";
const INSTALL_DIRECTORY: &str = "/usr/libexec/keeless";
const INSTALLED_BINARY: &str = "/usr/libexec/keeless/keeless-passkey-linux";
const UNIT_DIRECTORY: &str = "/etc/systemd/system";
const UNIT_PREFIX: &str = "keeless-passkey";

pub fn current_user_id() -> u32 {
    unsafe { libc::getuid() }
}

pub fn service_name(user_id: u32) -> String {
    format!("{UNIT_PREFIX}@{user_id}.service")
}

pub fn enable(desktop: Option<PathBuf>) -> Result<(), String> {
    let user_id = current_user_id();
    if user_id == 0 {
        return Err("run --enable as the desktop user, not root".into());
    }
    elevate("--internal-install-service", user_id, desktop.as_deref())
}

pub fn disable() -> Result<(), String> {
    let user_id = current_user_id();
    if user_id == 0 {
        return Err("run --disable as the desktop user, not root".into());
    }
    elevate("--internal-disable-service", user_id, None)
}

/// This is only reachable through pkexec. Keeping the target UID explicit
/// prevents the root process from accidentally creating a root-owned daemon.
pub fn install_for_user(user_id: u32, desktop: Option<PathBuf>) -> Result<(), String> {
    require_root(user_id)?;
    prepare_uhid()?;
    install_binary()?;
    let unit_path = unit_path(user_id);
    fs::write(&unit_path, unit_file(user_id, desktop.as_deref())?)
        .map_err(|error| format!("cannot write {}: {error}", unit_path.display()))?;
    set_mode(&unit_path, 0o644)?;
    systemctl(["daemon-reload"])?;
    systemctl(["enable", "--now", &service_name(user_id)])
}

pub fn disable_for_user(user_id: u32) -> Result<(), String> {
    require_root(user_id)?;
    if !unit_path(user_id).exists() {
        return Ok(());
    }
    systemctl(["disable", "--now", &service_name(user_id)])
}

fn require_root(user_id: u32) -> Result<(), String> {
    if current_user_id() != 0 {
        return Err("the internal service command must be run through pkexec".into());
    }
    if user_id == 0 {
        return Err("the passkey service must run as a non-root desktop user".into());
    }
    Ok(())
}

fn elevate(action: &str, user_id: u32, desktop: Option<&Path>) -> Result<(), String> {
    let mut command = Command::new("pkexec");
    command
        .arg("/proc/self/exe")
        .arg(action)
        .arg("--uid")
        .arg(user_id.to_string());
    if let Some(desktop) = desktop {
        command.arg("--desktop").arg(desktop);
    }
    let output = command
        .output()
        .map_err(|error| format!("cannot start pkexec: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    if output.status.code() == Some(126) {
        return Err("Authentication was cancelled.".into());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if detail.is_empty() {
        Err("polkit authentication failed".into())
    } else {
        Err(detail)
    }
}

fn prepare_uhid() -> Result<(), String> {
    fs::write(MODULE_CONFIG_PATH, "uhid\n")
        .map_err(|error| format!("cannot write {MODULE_CONFIG_PATH}: {error}"))?;
    set_mode(Path::new(MODULE_CONFIG_PATH), 0o644)?;
    run("modprobe", ["uhid"])?;
    if !Path::new("/dev/uhid").exists() {
        return Err("the uhid module loaded but /dev/uhid is unavailable".into());
    }
    Ok(())
}

fn install_binary() -> Result<(), String> {
    fs::create_dir_all(INSTALL_DIRECTORY)
        .map_err(|error| format!("cannot create {INSTALL_DIRECTORY}: {error}"))?;
    fs::copy("/proc/self/exe", INSTALLED_BINARY)
        .map_err(|error| format!("cannot install passkey service binary: {error}"))?;
    set_mode(Path::new(INSTALLED_BINARY), 0o755)
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| format!("cannot set permissions on {}: {error}", path.display()))
}

fn unit_path(user_id: u32) -> PathBuf {
    Path::new(UNIT_DIRECTORY).join(service_name(user_id))
}

fn unit_file(user_id: u32, desktop: Option<&Path>) -> Result<String, String> {
    let desktop = desktop.map(systemd_argument).transpose()?;
    let desktop = desktop
        .map(|path| format!(" --desktop {path}"))
        .unwrap_or_default();
    Ok(format!(
        "[Unit]\nDescription=Keeless Passkey Provider\nAfter=user@{user_id}.service\nRequires=user@{user_id}.service\n\n[Service]\nType=simple\nUser={user_id}\nEnvironment=XDG_RUNTIME_DIR=/run/user/{user_id}\nExecStart={INSTALLED_BINARY} run{desktop}\nOpenFile=/dev/uhid:uhid\nRestart=on-failure\nRestartSec=1\n\n[Install]\nWantedBy=multi-user.target\n"
    ))
}

fn systemd_argument(path: &Path) -> Result<String, String> {
    let path = path
        .to_str()
        .ok_or_else(|| "desktop executable path must be valid UTF-8 for systemd".to_string())?;
    if path.contains(['\0', '\n', '\r']) {
        return Err("desktop executable path contains an unsupported control character".into());
    }
    Ok(format!(
        "\"{}\"",
        path.replace('\\', "\\\\").replace('\"', "\\\"")
    ))
}

fn systemctl<const N: usize>(arguments: [&str; N]) -> Result<(), String> {
    run("systemctl", arguments)
}

fn run<const N: usize>(program: &str, arguments: [&str; N]) -> Result<(), String> {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .map_err(|error| format!("cannot run {program}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if detail.is_empty() {
        Err(format!("{program} exited with {}", output.status))
    } else {
        Err(format!("{program}: {detail}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_runs_the_daemon_as_the_requested_user() {
        let unit = unit_file(1000, Some(Path::new("/opt/Keeless App/keeless"))).unwrap();
        assert!(unit.contains("User=1000\n"));
        assert!(unit.contains("OpenFile=/dev/uhid:uhid\n"));
        assert!(unit.contains("ExecStart=/usr/libexec/keeless/keeless-passkey-linux run --desktop \"/opt/Keeless App/keeless\""));
    }

    #[test]
    fn unit_rejects_desktop_path_injection() {
        assert!(systemd_argument(Path::new("/opt/keeless\nUser=root")).is_err());
    }
}
