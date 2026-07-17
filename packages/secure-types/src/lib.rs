//! Byte-oriented secure containers used by Keeless.
//!
//! This is a deliberately narrowed fork of `secure-types` 0.3.0. On Linux,
//! allocations use dedicated anonymous mappings whose lock failures are
//! reported to the caller. Other targets retain zeroization-on-drop.

use std::fmt;

pub use zeroize::Zeroize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    AllocationFailed,
    LockFailed(i32),
    DumpProtectionFailed(i32),
    LengthMismatch,
    InvalidUtf8,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed => f.write_str("failed to allocate secure memory"),
            Self::LockFailed(code) => write!(f, "failed to lock secure memory (OS error {code})"),
            Self::DumpProtectionFailed(code) => {
                write!(
                    f,
                    "failed to exclude secure memory from dumps (OS error {code})"
                )
            }
            Self::LengthMismatch => f.write_str("secure buffer length mismatch"),
            Self::InvalidUtf8 => f.write_str("secure bytes are not valid UTF-8"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(all(feature = "use_os", target_os = "linux"))]
mod storage {
    use std::ptr::NonNull;
    use std::sync::atomic::{Ordering, compiler_fence};

    use super::Error;

    pub(super) struct Storage {
        ptr: NonNull<u8>,
        len: usize,
        allocation_len: usize,
    }

    impl Storage {
        pub(super) fn from_slice(value: &[u8]) -> Result<Self, Error> {
            let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
            if page_size <= 0 {
                return Err(Error::AllocationFailed);
            }
            let page_size = page_size as usize;
            let requested = value.len().max(1);
            let allocation_len = requested
                .checked_add(page_size - 1)
                .map(|size| size / page_size * page_size)
                .ok_or(Error::AllocationFailed)?;

            let raw = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    allocation_len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                )
            };
            if raw == libc::MAP_FAILED {
                return Err(Error::AllocationFailed);
            }
            let Some(ptr) = NonNull::new(raw.cast::<u8>()) else {
                unsafe {
                    libc::munmap(raw, allocation_len);
                }
                return Err(Error::AllocationFailed);
            };

            if unsafe { libc::mlock(raw, allocation_len) } != 0 {
                let code = std::io::Error::last_os_error().raw_os_error().unwrap_or(-1);
                unsafe {
                    libc::munmap(raw, allocation_len);
                }
                return Err(Error::LockFailed(code));
            }

            if unsafe { libc::madvise(raw, allocation_len, libc::MADV_DONTDUMP) } != 0 {
                let code = std::io::Error::last_os_error().raw_os_error().unwrap_or(-1);
                unsafe {
                    libc::munlock(raw, allocation_len);
                    libc::munmap(raw, allocation_len);
                }
                return Err(Error::DumpProtectionFailed(code));
            }

            unsafe {
                std::ptr::copy_nonoverlapping(value.as_ptr(), ptr.as_ptr(), value.len());
            }

            Ok(Self {
                ptr,
                len: value.len(),
                allocation_len,
            })
        }

        pub(super) fn as_slice(&self) -> &[u8] {
            unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
        }

        pub(super) fn as_mut_slice(&mut self) -> &mut [u8] {
            unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
        }
    }

    impl Drop for Storage {
        fn drop(&mut self) {
            unsafe {
                for index in 0..self.allocation_len {
                    std::ptr::write_volatile(self.ptr.as_ptr().add(index), 0);
                }
                compiler_fence(Ordering::SeqCst);
                libc::madvise(
                    self.ptr.as_ptr().cast(),
                    self.allocation_len,
                    libc::MADV_DODUMP,
                );
                libc::munlock(self.ptr.as_ptr().cast(), self.allocation_len);
                libc::munmap(self.ptr.as_ptr().cast(), self.allocation_len);
            }
        }
    }

    unsafe impl Send for Storage {}
    unsafe impl Sync for Storage {}
}

#[cfg(not(all(feature = "use_os", target_os = "linux")))]
mod storage {
    use zeroize::Zeroize;

    use super::Error;

    pub(super) struct Storage(Box<[u8]>);

    impl Storage {
        pub(super) fn from_slice(value: &[u8]) -> Result<Self, Error> {
            Ok(Self(value.into()))
        }

        pub(super) fn as_slice(&self) -> &[u8] {
            &self.0
        }

        pub(super) fn as_mut_slice(&mut self) -> &mut [u8] {
            &mut self.0
        }
    }

    impl Drop for Storage {
        fn drop(&mut self) {
            self.0.zeroize();
        }
    }
}

use storage::Storage;

pub struct SecureBytes(Storage);

impl SecureBytes {
    pub fn new() -> Result<Self, Error> {
        Self::from_slice(&[])
    }

    pub fn from_slice(value: &[u8]) -> Result<Self, Error> {
        Storage::from_slice(value).map(Self)
    }

    pub fn from_vec(mut value: Vec<u8>) -> Result<Self, Error> {
        let result = Self::from_slice(&value);
        value.zeroize();
        result
    }

    pub fn len(&self) -> usize {
        self.0.as_slice().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn unlock_slice<R>(&self, use_value: impl FnOnce(&[u8]) -> R) -> R {
        use_value(self.0.as_slice())
    }

    pub fn unlock_slice_mut<R>(&mut self, use_value: impl FnOnce(&mut [u8]) -> R) -> R {
        use_value(self.0.as_mut_slice())
    }

    pub fn erase(&mut self) {
        self.0.as_mut_slice().zeroize();
    }

    pub fn try_clone(&self) -> Result<Self, Error> {
        Self::from_slice(self.0.as_slice())
    }
}

pub struct SecureArray<const LENGTH: usize>(SecureBytes);

impl<const LENGTH: usize> SecureArray<LENGTH> {
    pub fn zeroed() -> Result<Self, Error> {
        Self::from_slice(&[0; LENGTH])
    }

    pub fn from_slice(value: &[u8]) -> Result<Self, Error> {
        if value.len() != LENGTH {
            return Err(Error::LengthMismatch);
        }
        Ok(Self(SecureBytes::from_slice(value)?))
    }

    pub fn from_array_mut(value: &mut [u8; LENGTH]) -> Result<Self, Error> {
        let result = Self::from_slice(value);
        value.zeroize();
        result
    }

    pub fn unlock<R>(&self, use_value: impl FnOnce(&[u8; LENGTH]) -> R) -> R {
        self.0.unlock_slice(|value| {
            use_value(value.try_into().expect("secure array length invariant"))
        })
    }

    pub fn unlock_mut<R>(&mut self, use_value: impl FnOnce(&mut [u8; LENGTH]) -> R) -> R {
        self.0.unlock_slice_mut(|value| {
            use_value(value.try_into().expect("secure array length invariant"))
        })
    }

    pub fn erase(&mut self) {
        self.0.erase();
    }

    pub fn try_clone(&self) -> Result<Self, Error> {
        self.unlock(|value| Self::from_slice(value))
    }
}

pub struct SecureString(SecureBytes);

impl SecureString {
    pub fn new() -> Result<Self, Error> {
        Ok(Self(SecureBytes::new()?))
    }

    pub fn from_text(value: &str) -> Result<Self, Error> {
        Ok(Self(SecureBytes::from_slice(value.as_bytes())?))
    }

    pub fn try_from_bytes(value: SecureBytes) -> Result<Self, Error> {
        if value.unlock_slice(|bytes| std::str::from_utf8(bytes).is_ok()) {
            Ok(Self(value))
        } else {
            Err(Error::InvalidUtf8)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn unlock_str<R>(&self, use_value: impl FnOnce(&str) -> R) -> R {
        self.0.unlock_slice(|value| {
            use_value(std::str::from_utf8(value).expect("secure string UTF-8 invariant"))
        })
    }

    pub fn erase(&mut self) {
        self.0.erase();
    }

    pub fn try_clone(&self) -> Result<Self, Error> {
        Ok(Self(self.0.try_clone()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_and_arrays_are_scoped() {
        let bytes = SecureBytes::from_slice(b"secret").unwrap();
        assert!(bytes.unlock_slice(|value| value == b"secret"));

        let mut input = [7u8; 32];
        let array = SecureArray::from_array_mut(&mut input).unwrap();
        assert_eq!(input, [0; 32]);
        assert!(array.unlock(|value| *value == [7; 32]));
    }

    #[test]
    fn secure_string_validates_utf8() {
        let value = SecureString::from_text("password").unwrap();
        assert_eq!(value.unlock_str(str::len), 8);
        assert!(SecureString::try_from_bytes(SecureBytes::from_slice(&[0xff]).unwrap()).is_err());
    }
}
