//! Diagnostics for the installed Linux passkey service.

use std::{collections::HashMap, fs, path::Path, process::Command};

use serde::Serialize;

use crate::{
    service::service_name,
    uhid::{DEVICE_NAME, PRODUCT_ID, VENDOR_ID},
};

const MINIMUM_SYSTEMD_VERSION: u32 = 253;

#[derive(Debug, Serialize)]
pub struct PasskeyState {
    pub platform: &'static str,
    pub state: State,
    pub enabled: bool,
    pub checks: Vec<PasskeyCheck>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Enabled,
    Disabled,
    Degraded,
    Unsupported,
}

#[derive(Debug, Serialize)]
pub struct PasskeyCheck {
    pub id: &'static str,
    pub label: &'static str,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Ok,
    Warning,
    Error,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ServiceStatus {
    installed: bool,
    enabled: bool,
    running: bool,
}

pub fn diagnose(user_id: u32) -> PasskeyState {
    let device = find_uhid_device(Path::new("/sys"));
    match systemd_support() {
        Err(error) => unsupported_state(error, device),
        Ok(()) => match service_status(user_id) {
            Err(error) => service_error_state(error, device),
            Ok(service) => state_from_checks(service, device),
        },
    }
}

pub fn print_human(state: &PasskeyState) {
    for check in &state.checks {
        println!("{}: {}", check.label, check.detail);
    }
}

fn unsupported_state(error: String, device: Result<bool, String>) -> PasskeyState {
    PasskeyState {
        platform: "linux",
        state: State::Unsupported,
        enabled: false,
        checks: vec![
            check("service", "Passkey service", CheckStatus::Error, error),
            device_check(device, false),
        ],
    }
}

fn service_error_state(error: String, device: Result<bool, String>) -> PasskeyState {
    PasskeyState {
        platform: "linux",
        state: State::Degraded,
        enabled: false,
        checks: vec![
            check("service", "Passkey service", CheckStatus::Error, error),
            check(
                "process",
                "Service process",
                CheckStatus::Warning,
                "unknown",
            ),
            device_check(device, false),
        ],
    }
}

fn state_from_checks(service: ServiceStatus, device: Result<bool, String>) -> PasskeyState {
    let device_present = device.as_ref().ok().copied().unwrap_or(false);
    let state = if service.enabled && service.running && device_present {
        State::Enabled
    } else if !service.enabled && !service.running && !device_present {
        State::Disabled
    } else {
        State::Degraded
    };
    let service_detail = if service.enabled {
        "enabled"
    } else if service.installed {
        "disabled"
    } else {
        "not installed"
    };
    let process_check = if service.running {
        check("process", "Service process", CheckStatus::Ok, "running")
    } else if service.enabled {
        check(
            "process",
            "Service process",
            CheckStatus::Error,
            "not running",
        )
    } else {
        check(
            "process",
            "Service process",
            CheckStatus::Warning,
            "stopped",
        )
    };

    PasskeyState {
        platform: "linux",
        state,
        enabled: service.enabled,
        checks: vec![
            check(
                "service",
                "Passkey service",
                CheckStatus::Ok,
                service_detail,
            ),
            process_check,
            device_check(device, service.enabled),
        ],
    }
}

fn device_check(device: Result<bool, String>, enabled: bool) -> PasskeyCheck {
    match device {
        Ok(true) => check("uhid", "Virtual FIDO device", CheckStatus::Ok, "available"),
        Ok(false) if enabled => check("uhid", "Virtual FIDO device", CheckStatus::Error, "missing"),
        Ok(false) => check(
            "uhid",
            "Virtual FIDO device",
            CheckStatus::Warning,
            "not present",
        ),
        Err(error) => check("uhid", "Virtual FIDO device", CheckStatus::Error, error),
    }
}

fn check(
    id: &'static str,
    label: &'static str,
    status: CheckStatus,
    detail: impl Into<String>,
) -> PasskeyCheck {
    PasskeyCheck {
        id,
        label,
        status,
        detail: detail.into(),
    }
}

fn systemd_support() -> Result<(), String> {
    let output = Command::new("systemctl")
        .arg("--version")
        .output()
        .map_err(|error| format!("systemd is unavailable: {error}"))?;
    if !output.status.success() {
        return Err("systemd is unavailable".into());
    }
    let output = String::from_utf8_lossy(&output.stdout);
    let version = systemd_version(&output)
        .ok_or_else(|| "cannot determine the installed systemd version".to_string())?;
    if version < MINIMUM_SYSTEMD_VERSION {
        return Err(format!(
            "systemd {MINIMUM_SYSTEMD_VERSION} or newer is required for OpenFile= (found {version})"
        ));
    }
    Ok(())
}

fn service_status(user_id: u32) -> Result<ServiceStatus, String> {
    let unit = service_name(user_id);
    let output = Command::new("systemctl")
        .args([
            "show",
            &unit,
            "--property=LoadState",
            "--property=UnitFileState",
            "--property=ActiveState",
            "--property=SubState",
        ])
        .output()
        .map_err(|error| format!("cannot inspect passkey service: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.is_empty() && !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            "cannot inspect passkey service".into()
        } else {
            detail
        });
    }
    Ok(parse_service_status(&stdout))
}

fn parse_service_status(output: &str) -> ServiceStatus {
    let properties = output
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect::<HashMap<_, _>>();
    let enabled = matches!(
        properties.get("UnitFileState").copied(),
        Some("enabled" | "enabled-runtime" | "linked" | "linked-runtime")
    );
    ServiceStatus {
        installed: properties.get("LoadState").copied() == Some("loaded"),
        enabled,
        running: properties.get("ActiveState").copied() == Some("active")
            && properties.get("SubState").copied() == Some("running"),
    }
}

/// Look for the hidraw device created by this provider, not the `/dev/uhid`
/// control device that creates it.
pub(crate) fn find_uhid_device(sysfs_root: &Path) -> Result<bool, String> {
    let entries = fs::read_dir(sysfs_root.join("class/hidraw"))
        .map_err(|error| format!("cannot inspect hidraw devices: {error}"))?;
    let id = format!(":{VENDOR_ID:08x}:{PRODUCT_ID:08x}");
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot inspect hidraw device: {error}"))?;
        let uevent = entry.path().join("device/uevent");
        let Ok(uevent) = fs::read_to_string(uevent) else {
            continue;
        };
        let name_matches = uevent
            .lines()
            .any(|line| line == format!("HID_NAME={DEVICE_NAME}"));
        let id_matches = uevent.lines().any(|line| {
            line.strip_prefix("HID_ID=")
                .is_some_and(|id_value| id_value.to_ascii_lowercase().ends_with(&id))
        });
        if name_matches && id_matches {
            return Ok(true);
        }
    }
    Ok(false)
}

fn systemd_version(output: &str) -> Option<u32> {
    output
        .lines()
        .next()?
        .strip_prefix("systemd ")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn maps_service_and_device_health_to_states() {
        let enabled = ServiceStatus {
            installed: true,
            enabled: true,
            running: true,
        };
        assert_eq!(state_from_checks(enabled, Ok(true)).state, State::Enabled);

        let disabled = ServiceStatus {
            installed: true,
            enabled: false,
            running: false,
        };
        assert_eq!(
            state_from_checks(disabled, Ok(false)).state,
            State::Disabled
        );

        let degraded = ServiceStatus {
            installed: true,
            enabled: true,
            running: true,
        };
        assert_eq!(
            state_from_checks(degraded, Ok(false)).state,
            State::Degraded
        );
    }

    #[test]
    fn parses_systemctl_properties_without_status_output() {
        let status = parse_service_status(
            "LoadState=loaded\nUnitFileState=enabled\nActiveState=active\nSubState=running\n",
        );
        assert_eq!(
            status,
            ServiceStatus {
                installed: true,
                enabled: true,
                running: true,
            }
        );
        assert_eq!(systemd_version("systemd 253 (253.1)\n"), Some(253));
    }

    #[test]
    fn detects_the_keeless_hidraw_device_in_sysfs() {
        let root = std::env::temp_dir().join(format!(
            "keeless-passkey-doctor-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let uevent = root.join("class/hidraw/hidraw0/device/uevent");
        fs::create_dir_all(uevent.parent().unwrap()).unwrap();
        fs::write(&uevent, "HID_NAME=Keeless\nHID_ID=0003:00001209:00005031\n").unwrap();
        assert!(find_uhid_device(&root).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}
