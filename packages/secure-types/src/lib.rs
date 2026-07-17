//! Byte-oriented secure containers used by Keeless.
//!
//! This is a deliberately narrowed fork of `secure-types` 0.3.0. On Linux,
//! allocations use dedicated anonymous mappings whose lock failures are
//! reported to the caller. On Windows, values are protected with DPAPI while
//! not in use. Other targets retain zeroization-on-drop.

use std::fmt;

pub use zeroize::Zeroize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    AllocationFailed,
    LockFailed(i32),
    DumpProtectionFailed(i32),
    ProtectMemoryFailed(u32),
    UnprotectMemoryFailed(u32),
    LengthTooLarge,
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
            Self::ProtectMemoryFailed(code) => {
                write!(f, "failed to protect secure memory (Windows error {code})")
            }
            Self::UnprotectMemoryFailed(code) => {
                write!(
                    f,
                    "failed to unprotect secure memory (Windows error {code})"
                )
            }
            Self::LengthTooLarge => f.write_str("secure buffer is too large to protect"),
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

        pub(super) fn len(&self) -> usize {
            self.len
        }

        pub(super) fn with_slice<R>(&self, use_value: impl FnOnce(&[u8]) -> R) -> Result<R, Error> {
            let value = unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) };
            Ok(use_value(value))
        }

        pub(super) fn with_mut_slice<R>(
            &mut self,
            use_value: impl FnOnce(&mut [u8]) -> R,
        ) -> Result<R, Error> {
            let value = unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) };
            Ok(use_value(value))
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

#[cfg(all(feature = "use_os", target_os = "windows"))]
mod storage {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPTPROTECTMEMORY_BLOCK_SIZE, CRYPTPROTECTMEMORY_SAME_PROCESS, CryptProtectMemory,
        CryptUnprotectMemory,
    };
    use zeroize::{Zeroize, Zeroizing};

    use super::Error;

    const BLOCK_SIZE: usize = CRYPTPROTECTMEMORY_BLOCK_SIZE as usize;
    const MAX_PROTECTED_LEN: usize = u32::MAX as usize / BLOCK_SIZE * BLOCK_SIZE;

    pub(super) struct Storage {
        protected: Box<[u8]>,
        len: usize,
    }

    impl Storage {
        pub(super) fn from_slice(value: &[u8]) -> Result<Self, Error> {
            let protected_len = protected_len(value.len())?;
            let mut protected = zeroed_vec(protected_len)?;
            protected[..value.len()].copy_from_slice(value);

            if let Err(error) = protect(&mut protected) {
                protected.zeroize();
                return Err(error);
            }

            Ok(Self {
                protected: protected.into_boxed_slice(),
                len: value.len(),
            })
        }

        pub(super) fn len(&self) -> usize {
            self.len
        }

        pub(super) fn with_slice<R>(&self, use_value: impl FnOnce(&[u8]) -> R) -> Result<R, Error> {
            let mut plaintext = Zeroizing::new(copy_buffer(&self.protected)?);
            unprotect(&mut plaintext)?;
            Ok(use_value(&plaintext[..self.len]))
        }

        pub(super) fn with_mut_slice<R>(
            &mut self,
            use_value: impl FnOnce(&mut [u8]) -> R,
        ) -> Result<R, Error> {
            let mut plaintext = Zeroizing::new(copy_buffer(&self.protected)?);
            unprotect(&mut plaintext)?;

            let result = use_value(&mut plaintext[..self.len]);
            plaintext[self.len..].zeroize();
            protect(&mut plaintext)?;

            self.protected.zeroize();
            self.protected.copy_from_slice(&plaintext);
            Ok(result)
        }
    }

    impl Drop for Storage {
        fn drop(&mut self) {
            self.protected.zeroize();
        }
    }

    fn protected_len(len: usize) -> Result<usize, Error> {
        let len = len.max(1);
        let padded = len
            .checked_add(BLOCK_SIZE - 1)
            .map(|value| value / BLOCK_SIZE * BLOCK_SIZE)
            .ok_or(Error::LengthTooLarge)?;
        if padded > MAX_PROTECTED_LEN {
            return Err(Error::LengthTooLarge);
        }
        Ok(padded)
    }

    fn zeroed_vec(len: usize) -> Result<Vec<u8>, Error> {
        let mut value = Vec::new();
        value
            .try_reserve_exact(len)
            .map_err(|_| Error::AllocationFailed)?;
        value.resize(len, 0);
        Ok(value)
    }

    fn copy_buffer(value: &[u8]) -> Result<Vec<u8>, Error> {
        let mut copy = Vec::new();
        copy.try_reserve_exact(value.len())
            .map_err(|_| Error::AllocationFailed)?;
        copy.extend_from_slice(value);
        Ok(copy)
    }

    fn protect(value: &mut [u8]) -> Result<(), Error> {
        let success = unsafe {
            CryptProtectMemory(
                value.as_mut_ptr().cast(),
                value.len() as u32,
                CRYPTPROTECTMEMORY_SAME_PROCESS,
            )
        };
        if success == 0 {
            return Err(Error::ProtectMemoryFailed(unsafe { GetLastError() }));
        }
        Ok(())
    }

    fn unprotect(value: &mut [u8]) -> Result<(), Error> {
        let success = unsafe {
            CryptUnprotectMemory(
                value.as_mut_ptr().cast(),
                value.len() as u32,
                CRYPTPROTECTMEMORY_SAME_PROCESS,
            )
        };
        if success == 0 {
            return Err(Error::UnprotectMemoryFailed(unsafe { GetLastError() }));
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn protected_lengths_are_checked() {
            assert_eq!(protected_len(0).unwrap(), 16);
            assert_eq!(protected_len(1).unwrap(), 16);
            assert_eq!(protected_len(16).unwrap(), 16);
            assert_eq!(protected_len(17).unwrap(), 32);
            assert_eq!(protected_len(MAX_PROTECTED_LEN).unwrap(), MAX_PROTECTED_LEN);
            assert_eq!(
                protected_len(MAX_PROTECTED_LEN + 1),
                Err(Error::LengthTooLarge)
            );
            assert_eq!(protected_len(usize::MAX), Err(Error::LengthTooLarge));
        }

        #[test]
        fn backing_storage_is_protected() {
            let storage = Storage::from_slice(b"secret").unwrap();
            assert_ne!(&storage.protected[..6], b"secret");
            assert_eq!(storage.protected.len(), BLOCK_SIZE);
        }
    }
}

#[cfg(all(
    feature = "use_os",
    not(any(target_os = "linux", target_os = "windows"))
))]
compile_error!("feature `use_os` is unsupported on this target");

#[cfg(not(feature = "use_os"))]
mod storage {
    use zeroize::Zeroize;

    use super::Error;

    pub(super) struct Storage(Box<[u8]>);

    impl Storage {
        pub(super) fn from_slice(value: &[u8]) -> Result<Self, Error> {
            Ok(Self(value.into()))
        }

        pub(super) fn len(&self) -> usize {
            self.0.len()
        }

        pub(super) fn with_slice<R>(&self, use_value: impl FnOnce(&[u8]) -> R) -> Result<R, Error> {
            Ok(use_value(&self.0))
        }

        pub(super) fn with_mut_slice<R>(
            &mut self,
            use_value: impl FnOnce(&mut [u8]) -> R,
        ) -> Result<R, Error> {
            Ok(use_value(&mut self.0))
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
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn unlock_slice<R>(&self, use_value: impl FnOnce(&[u8]) -> R) -> Result<R, Error> {
        self.0.with_slice(use_value)
    }

    pub fn unlock_slice_mut<R>(
        &mut self,
        use_value: impl FnOnce(&mut [u8]) -> R,
    ) -> Result<R, Error> {
        self.0.with_mut_slice(use_value)
    }

    pub fn erase(&mut self) -> Result<(), Error> {
        self.unlock_slice_mut(Zeroize::zeroize)
    }

    pub fn try_clone(&self) -> Result<Self, Error> {
        self.unlock_slice(Self::from_slice)?
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

    pub fn unlock<R>(&self, use_value: impl FnOnce(&[u8; LENGTH]) -> R) -> Result<R, Error> {
        self.0.unlock_slice(|value| {
            use_value(value.try_into().expect("secure array length invariant"))
        })
    }

    pub fn unlock_mut<R>(
        &mut self,
        use_value: impl FnOnce(&mut [u8; LENGTH]) -> R,
    ) -> Result<R, Error> {
        self.0.unlock_slice_mut(|value| {
            use_value(value.try_into().expect("secure array length invariant"))
        })
    }

    pub fn erase(&mut self) -> Result<(), Error> {
        self.0.erase()
    }

    pub fn try_clone(&self) -> Result<Self, Error> {
        self.unlock(|value| Self::from_slice(value))?
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
        if value.unlock_slice(|bytes| std::str::from_utf8(bytes).is_ok())? {
            Ok(Self(value))
        } else {
            Err(Error::InvalidUtf8)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn unlock_str<R>(&self, use_value: impl FnOnce(&str) -> R) -> Result<R, Error> {
        self.0.unlock_slice(|value| {
            use_value(std::str::from_utf8(value).expect("secure string UTF-8 invariant"))
        })
    }

    pub fn erase(&mut self) -> Result<(), Error> {
        self.0.erase()
    }

    pub fn try_clone(&self) -> Result<Self, Error> {
        Ok(Self(self.0.try_clone()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn bytes_and_arrays_are_scoped() {
        let bytes = SecureBytes::from_slice(b"secret").unwrap();
        assert!(bytes.unlock_slice(|value| value == b"secret").unwrap());

        let mut input = [7u8; 32];
        let array = SecureArray::from_array_mut(&mut input).unwrap();
        assert_eq!(input, [0; 32]);
        assert!(array.unlock(|value| *value == [7; 32]).unwrap());
    }

    #[test]
    fn secure_string_validates_utf8() {
        let value = SecureString::from_text("password").unwrap();
        assert_eq!(value.unlock_str(str::len).unwrap(), 8);
        assert!(SecureString::try_from_bytes(SecureBytes::from_slice(&[0xff]).unwrap()).is_err());
    }

    #[test]
    fn bytes_preserve_logical_lengths() {
        for len in [0, 1, 15, 16, 17, 31, 32, 33] {
            let input = vec![0x5a; len];
            let value = SecureBytes::from_slice(&input).unwrap();
            assert_eq!(value.len(), len);
            assert_eq!(value.unlock_slice(|bytes| bytes.to_vec()).unwrap(), input);
        }
    }

    #[test]
    fn mutable_access_clone_and_erase_round_trip() {
        let mut value = SecureBytes::from_slice(b"secret").unwrap();
        value
            .unlock_slice_mut(|bytes| bytes.copy_from_slice(b"public"))
            .unwrap();

        let clone = value.try_clone().unwrap();
        assert_eq!(
            clone.unlock_slice(|bytes| bytes.to_vec()).unwrap(),
            b"public"
        );

        value.erase().unwrap();
        assert!(
            value
                .unlock_slice(|bytes| bytes.iter().all(|byte| *byte == 0))
                .unwrap()
        );
    }

    #[test]
    fn secure_containers_are_send_and_sync() {
        assert_send_sync::<SecureBytes>();
        assert_send_sync::<SecureArray<32>>();
        assert_send_sync::<SecureString>();
    }

    #[cfg(all(feature = "use_os", target_os = "windows"))]
    #[test]
    fn windows_storage_survives_callback_panics() {
        let value = SecureBytes::from_slice(b"secret").unwrap();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = value.unlock_slice(|_| panic!("callback panic"));
        }));

        assert!(panic.is_err());
        assert!(value.unlock_slice(|bytes| bytes == b"secret").unwrap());
    }

    #[cfg(all(feature = "use_os", target_os = "windows"))]
    #[test]
    fn windows_storage_supports_concurrent_readers() {
        use std::sync::Arc;

        let value = Arc::new(SecureBytes::from_slice(b"secret").unwrap());
        let readers: Vec<_> = (0..8)
            .map(|_| {
                let value = Arc::clone(&value);
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        assert!(value.unlock_slice(|bytes| bytes == b"secret").unwrap());
                    }
                })
            })
            .collect();

        for reader in readers {
            reader.join().unwrap();
        }
    }
}
