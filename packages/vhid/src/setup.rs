//! One-time host configuration for the virtual HID device.
//!
//! `/dev/uhid` is root-owned, so a desktop session cannot create a virtual device
//! without a udev rule granting the logged-in user access. Installing that rule
//! needs privilege the daemon deliberately does not have, so `setup` prints the
//! commands and `doctor` reports what is still missing.

use std::path::Path;

pub const UDEV_RULE_PATH: &str = "/etc/udev/rules.d/70-keeless-uhid.rules";
pub const MODULE_CONFIG_PATH: &str = "/etc/modules-load.d/keeless-uhid.conf";

/// Grants the user of the active local session access to `/dev/uhid`.
///
/// `static_node` covers the case where uhid is built in rather than loaded as a
/// module, when no add event ever fires to apply the rule.
pub const UDEV_RULE: &str =
    "KERNEL==\"uhid\", SUBSYSTEM==\"misc\", TAG+=\"uaccess\", OPTIONS+=\"static_node=uhid\"\n";

pub const MODULE_CONFIG: &str = "uhid\n";

/// What stands between the daemon and a working virtual device.
#[derive(Debug, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    DeviceMissing,
    PermissionDenied,
}

impl Readiness {
    pub fn describe(&self) -> &'static str {
        match self {
            Self::Ready => "/dev/uhid is present and writable",
            Self::DeviceMissing => "/dev/uhid does not exist; the uhid module is not loaded",
            Self::PermissionDenied => "/dev/uhid exists but is not writable by this user",
        }
    }
}

/// Check whether this user can create a virtual HID device right now.
pub fn readiness() -> Readiness {
    let path = Path::new("/dev/uhid");
    if !path.exists() {
        return Readiness::DeviceMissing;
    }
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
    {
        Ok(_) => Readiness::Ready,
        Err(_) => Readiness::PermissionDenied,
    }
}

/// Print the commands that make `/dev/uhid` usable.
///
/// Printing rather than running them keeps the daemon out of the privilege
/// business: the user sees exactly what will be written before it happens, and
/// packaging formats without an install step still have a documented path.
pub fn print_instructions() {
    println!("keeless-vhid needs access to /dev/uhid. Run, as root:\n");
    println!("  install -m 0644 /dev/stdin {UDEV_RULE_PATH} <<'RULE'");
    print!("{UDEV_RULE}");
    println!("RULE");
    println!("  install -m 0644 /dev/stdin {MODULE_CONFIG_PATH} <<'CONF'");
    print!("{MODULE_CONFIG}");
    println!("CONF");
    println!("  modprobe uhid");
    println!("  udevadm control --reload-rules");
    println!("  udevadm trigger --name-match=uhid");
    println!("\nThen log out and back in, or replug your session, so the new");
    println!("access rule applies. Check it with `keeless-vhid doctor`.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_udev_rule_tags_uhid_for_session_access() {
        assert!(UDEV_RULE.contains("KERNEL==\"uhid\""));
        assert!(UDEV_RULE.contains("TAG+=\"uaccess\""));
        assert!(UDEV_RULE.contains("static_node=uhid"));
        assert!(UDEV_RULE.ends_with('\n'));
    }

    #[test]
    fn every_readiness_state_explains_itself() {
        for state in [
            Readiness::Ready,
            Readiness::DeviceMissing,
            Readiness::PermissionDenied,
        ] {
            assert!(state.describe().contains("/dev/uhid"));
        }
    }
}
