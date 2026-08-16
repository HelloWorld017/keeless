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
pub fn set_directory_permissions(path: &Path) -> io::Result<()> {
    set_owner_only_dacl(path)
}

#[cfg(unix)]
pub fn set_private_create_mode(options: &mut tokio::fs::OpenOptions) {
    options.mode(0o600);
}

#[cfg(windows)]
pub fn set_private_create_mode(_: &mut tokio::fs::OpenOptions) {}

#[cfg(unix)]
pub async fn set_file_permissions(file: &tokio::fs::File, _: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .await
}

#[cfg(windows)]
pub async fn set_file_permissions(_: &tokio::fs::File, path: &Path) -> io::Result<()> {
    set_owner_only_dacl(path)
}

#[cfg(windows)]
pub fn ensure_file_permissions(path: &Path) -> io::Result<()> {
    set_owner_only_dacl(path)
}

#[cfg(windows)]
fn set_owner_only_dacl(path: &Path) -> io::Result<()> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};

    const OWNER_SECURITY_INFORMATION: u32 = 0x0000_0001;
    const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
    const SDDL_REVISION_1: u32 = 1;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
            string_security_descriptor: *const u16,
            string_sd_revision: u32,
            security_descriptor: *mut *mut c_void,
            security_descriptor_size: *mut u32,
        ) -> i32;
        fn SetFileSecurityW(
            file_name: *const u16,
            security_information: u32,
            security_descriptor: *const c_void,
        ) -> i32;
        fn OpenProcessToken(
            process: *mut c_void,
            desired_access: u32,
            token: *mut *mut c_void,
        ) -> i32;
        fn GetTokenInformation(
            token: *mut c_void,
            token_information_class: u32,
            token_information: *mut c_void,
            token_information_length: u32,
            return_length: *mut u32,
        ) -> i32;
        fn ConvertSidToStringSidW(sid: *mut c_void, string_sid: *mut *mut u16) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
        fn GetCurrentProcess() -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    #[repr(C)]
    struct SidAndAttributes {
        sid: *mut c_void,
        attributes: u32,
    }
    #[repr(C)]
    struct TokenUser {
        user: SidAndAttributes,
    }

    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_USER: u32 = 1;

    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let sddl = (|| {
        let mut bytes = 0_u32;
        unsafe { GetTokenInformation(token, TOKEN_USER, ptr::null_mut(), 0, &mut bytes) };
        if bytes < std::mem::size_of::<TokenUser>() as u32 {
            return Err(io::Error::last_os_error());
        }
        let words = (bytes as usize).div_ceil(std::mem::size_of::<usize>());
        let mut information = vec![0_usize; words];
        if unsafe {
            GetTokenInformation(
                token,
                TOKEN_USER,
                information.as_mut_ptr().cast(),
                bytes,
                &mut bytes,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = unsafe { &*(information.as_ptr().cast::<TokenUser>()) };
        if user.user.sid.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "token has no user SID",
            ));
        }
        let mut string_sid = ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(user.user.sid, &mut string_sid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let sid = unsafe { utf16_nul_terminated(string_sid, 256) };
        unsafe { LocalFree(string_sid.cast()) };
        let sid = sid?;
        // Explicitly set both the owner and protected DACL to this process's
        // token user. `OW` is insufficient: it follows a potentially foreign
        // pre-existing object owner.
        Ok(format!("O:{sid}D:P(A;;FA;;;{sid})")
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>())
    })();
    unsafe { CloseHandle(token) };
    let sddl = sddl?;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut descriptor = ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let result = unsafe {
        SetFileSecurityW(
            path.as_ptr(),
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    unsafe { LocalFree(descriptor) };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
unsafe fn utf16_nul_terminated(pointer: *const u16, maximum: usize) -> io::Result<String> {
    if pointer.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows API returned a null UTF-16 string",
        ));
    }
    for length in 0..maximum {
        if unsafe { *pointer.add(length) } == 0 {
            return String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, length) })
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid Windows SID"));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Windows SID is too long",
    ))
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
compile_error!("keeless_host_desktop_shared supports Unix and Windows only");
