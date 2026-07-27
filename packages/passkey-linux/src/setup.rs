//! One-time host configuration for the virtual HID device.
//!
//! `/dev/uhid` is root-owned, so a desktop session cannot create a virtual device
//! without a udev rule granting the logged-in user access. Installing that rule
//! needs privilege the daemon deliberately does not have, so `setup` prints the
//! commands and `doctor` reports what is still missing.

use std::path::Path;

pub const UDEV_RULE_PATH: &str = "/etc/udev/rules.d/70-keeless-uhid.rules";
pub const MODULE_CONFIG_PATH: &str = "/etc/modules-load.d/keeless-uhid.conf";

/// Group the rule grants `/dev/uhid` to.
pub const ACCESS_GROUP: &str = "keeless-uhid";

/// Grants members of [`ACCESS_GROUP`] access to `/dev/uhid`.
///
/// Deliberately a named group rather than `TAG+="uaccess"`. Writing to
/// `/dev/uhid` creates arbitrary virtual input devices, including keyboards, so
/// it is a keystroke-injection primitive — `uaccess` would hand that to every
/// process of every user who logs in at the console, forever. A group makes the
/// grant explicit and revocable.
///
/// `static_node` applies the mode when uhid is built into the kernel, where no
/// device event ever fires to trigger the rule.
pub const UDEV_RULE: &str = concat!(
    "KERNEL==\"uhid\", SUBSYSTEM==\"misc\", GROUP=\"keeless-uhid\", MODE=\"0660\", ",
    "OPTIONS+=\"static_node=uhid\"\n"
);

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
    println!("keeless-passkey-linux needs access to /dev/uhid.\n");
    println!("Be aware of what that grants: writing to /dev/uhid creates virtual");
    println!("input devices of any kind, keyboards included, so anyone holding it");
    println!("can type into your session. The commands below limit it to members");
    println!("of the {ACCESS_GROUP} group rather than to every logged-in user.\n");
    println!("Run, as root:\n");
    println!("  groupadd -f {ACCESS_GROUP}");
    println!("  gpasswd -a \"$USER\" {ACCESS_GROUP}");
    println!("  install -m 0644 /dev/stdin {UDEV_RULE_PATH} <<'RULE'");
    print!("{UDEV_RULE}");
    println!("RULE");
    println!("  install -m 0644 /dev/stdin {MODULE_CONFIG_PATH} <<'CONF'");
    print!("{MODULE_CONFIG}");
    println!("CONF");
    println!("  modprobe uhid");
    println!("  udevadm control --reload-rules");
    println!("  udevadm trigger --name-match=uhid");
    println!("\nThen log out and back in so the new group membership applies.");
    println!("Check the result with `keeless-passkey-linux doctor`.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_udev_rule_grants_uhid_to_one_group_and_nothing_else() {
        assert!(UDEV_RULE.contains("KERNEL==\"uhid\""));
        assert!(UDEV_RULE.contains("SUBSYSTEM==\"misc\""));
        assert!(UDEV_RULE.contains(&format!("GROUP=\"{ACCESS_GROUP}\"")));
        assert!(UDEV_RULE.contains("MODE=\"0660\""));
        assert!(UDEV_RULE.contains("static_node=uhid"));
        assert!(
            !UDEV_RULE.contains("uaccess"),
            "uaccess would hand virtual-keyboard creation to every console user"
        );
        assert!(
            !UDEV_RULE.contains("0666"),
            "the device must not be world-writable"
        );
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
