//! Single-instance guard.
//!
//! Two daemons would present two virtual devices and race for the same consent
//! prompts, so the second one exits instead.

use std::fs::File;
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

/// Held for the lifetime of the daemon; the lock is released when it drops.
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    /// Take the lock, or report that another daemon already holds it.
    pub fn acquire() -> io::Result<Option<Self>> {
        let path = lock_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::create(&path)?;
        // A lock held by an open file descriptor is released even if the process
        // is killed, so a crashed daemon never blocks the next one.
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if locked != 0 {
            let error = io::Error::last_os_error();
            return match error.raw_os_error() {
                Some(libc::EWOULDBLOCK) => Ok(None),
                _ => Err(error),
            };
        }
        Ok(Some(Self { _file: file }))
    }
}

/// Beside the app's IPC socket, in the per-user runtime directory.
fn lock_path() -> PathBuf {
    let root = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "{}-{}",
                keeless_host_client::ipc::NAMESPACE,
                unsafe { libc::geteuid() }
            ))
        });
    root.join(keeless_host_client::ipc::NAMESPACE)
        .join("vhid.lock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lock_sits_beside_the_app_endpoint() {
        let path = lock_path();
        assert!(path.ends_with("vhid.lock"));
        assert_eq!(
            path.parent().and_then(|parent| parent.file_name()),
            Some(std::ffi::OsStr::new(keeless_host_client::ipc::NAMESPACE))
        );
    }

    #[test]
    fn a_second_acquisition_in_this_process_reports_the_first() {
        // flock is per open file description, so a second `File::create` of the
        // same path contends with the first even within one process.
        let Ok(Some(first)) = InstanceLock::acquire() else {
            // The runtime directory may be unavailable in a sandbox; nothing to test.
            return;
        };
        assert!(matches!(InstanceLock::acquire(), Ok(None)));
        drop(first);
        assert!(matches!(InstanceLock::acquire(), Ok(Some(_))));
    }
}
