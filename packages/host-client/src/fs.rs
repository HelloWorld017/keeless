//! Atomic, owner-only file replacement shared by every process that persists
//! Keeless state next to the desktop app.

use std::io;
use std::path::{Path, PathBuf};

/// A sibling temporary path for a write-then-rename replacement.
pub fn temporary_path(target: &Path) -> io::Result<PathBuf> {
    let mut random = [0_u8; 16];
    getrandom::getrandom(&mut random).map_err(io::Error::other)?;
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no file name"))?;
    Ok(target.with_file_name(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        hex::encode(random)
    )))
}

#[cfg(unix)]
pub fn set_directory_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(windows)]
pub fn set_directory_permissions(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
pub fn set_private_create_mode(options: &mut tokio::fs::OpenOptions) {
    options.mode(0o600);
}

#[cfg(windows)]
pub fn set_private_create_mode(_: &mut tokio::fs::OpenOptions) {}

#[cfg(unix)]
pub async fn set_file_permissions(file: &tokio::fs::File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .await
}

#[cfg(windows)]
pub async fn set_file_permissions(_: &tokio::fs::File) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
pub fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

#[cfg(windows)]
pub fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(unix)]
pub fn sync_directory(path: &Path) -> io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

#[cfg(windows)]
pub fn sync_directory(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(not(any(unix, windows)))]
compile_error!("keeless_host_client supports Unix and Windows only");
