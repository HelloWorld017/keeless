//! Runtime resolution and ownership wrappers for `webauthn.dll`.

use std::{
    ffi::{CString, c_void},
    mem, ptr,
};

use windows_sys::Win32::{
    Foundation::{FreeLibrary, HMODULE},
    System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
};

use crate::sdk_bindings::*;

const E_NOTIMPL: HResult = 0x8000_4001_u32 as i32;

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoTaskMemFree(memory: *const c_void);
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Windows WebAuthn plugin APIs are not available")]
    Unsupported,
    #[error("webauthn.dll does not export {0}")]
    MissingExport(&'static str),
    #[error("Windows WebAuthn API failed with HRESULT {0:#x}")]
    HResult(HResult),
    #[error("Windows API returned an invalid buffer")]
    InvalidBuffer,
}

impl ApiError {
    pub fn hresult(&self) -> HResult {
        match self {
            Self::Unsupported | Self::MissingExport(_) => E_NOTIMPL,
            Self::HResult(value) => *value,
            Self::InvalidBuffer => crate::error::NTE_INVALID_PARAMETER,
        }
    }
}

/// Function table resolved from the system copy of `webauthn.dll`.
pub struct Api {
    module: HMODULE,
    pub get_authenticator_state: WebAuthNPluginGetAuthenticatorState,
    pub add_authenticator: WebAuthNPluginAddAuthenticator,
    pub free_add_authenticator_response: WebAuthNPluginFreeAddAuthenticatorResponse,
    pub remove_authenticator: WebAuthNPluginRemoveAuthenticator,
    pub perform_user_verification: WebAuthNPluginPerformUserVerification,
    pub free_user_verification_response: WebAuthNPluginFreeUserVerificationResponse,
    pub get_user_verification_public_key: WebAuthNPluginGetUserVerificationPublicKey,
    pub get_operation_signing_public_key: WebAuthNPluginGetOperationSigningPublicKey,
    pub free_public_key_response: WebAuthNPluginFreePublicKeyResponse,
    pub decode_make_credential_request: WebAuthNDecodeMakeCredentialRequest,
    pub free_decoded_make_credential_request: WebAuthNFreeDecodedMakeCredentialRequest,
    pub encode_make_credential_response: WebAuthNEncodeMakeCredentialResponse,
    pub decode_get_assertion_request: WebAuthNDecodeGetAssertionRequest,
    pub free_decoded_get_assertion_request: WebAuthNFreeDecodedGetAssertionRequest,
    pub encode_get_assertion_response: WebAuthNEncodeGetAssertionResponse,
    pub register_status_change_callback: WebAuthNPluginRegisterStatusChangeCallback,
    pub unregister_status_change_callback: WebAuthNPluginUnregisterStatusChangeCallback,
}

impl Api {
    pub fn load() -> Result<Self, ApiError> {
        if !supported_windows_build() {
            return Err(ApiError::Unsupported);
        }
        let name: Vec<u16> = "webauthn.dll".encode_utf16().chain(Some(0)).collect();
        let module =
            unsafe { LoadLibraryExW(name.as_ptr(), ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        if module.is_null() {
            return Err(ApiError::Unsupported);
        }

        let result = unsafe {
            Ok(Self {
                module,
                get_authenticator_state: symbol(module, "WebAuthNPluginGetAuthenticatorState")?,
                add_authenticator: symbol(module, "WebAuthNPluginAddAuthenticator")?,
                free_add_authenticator_response: symbol(
                    module,
                    "WebAuthNPluginFreeAddAuthenticatorResponse",
                )?,
                remove_authenticator: symbol(module, "WebAuthNPluginRemoveAuthenticator")?,
                perform_user_verification: symbol(module, "WebAuthNPluginPerformUserVerification")?,
                free_user_verification_response: symbol(
                    module,
                    "WebAuthNPluginFreeUserVerificationResponse",
                )?,
                get_user_verification_public_key: symbol(
                    module,
                    "WebAuthNPluginGetUserVerificationPublicKey",
                )?,
                get_operation_signing_public_key: symbol(
                    module,
                    "WebAuthNPluginGetOperationSigningPublicKey",
                )?,
                free_public_key_response: symbol(module, "WebAuthNPluginFreePublicKeyResponse")?,
                decode_make_credential_request: symbol(
                    module,
                    "WebAuthNDecodeMakeCredentialRequest",
                )?,
                free_decoded_make_credential_request: symbol(
                    module,
                    "WebAuthNFreeDecodedMakeCredentialRequest",
                )?,
                encode_make_credential_response: symbol(
                    module,
                    "WebAuthNEncodeMakeCredentialResponse",
                )?,
                decode_get_assertion_request: symbol(module, "WebAuthNDecodeGetAssertionRequest")?,
                free_decoded_get_assertion_request: symbol(
                    module,
                    "WebAuthNFreeDecodedGetAssertionRequest",
                )?,
                encode_get_assertion_response: symbol(
                    module,
                    "WebAuthNEncodeGetAssertionResponse",
                )?,
                register_status_change_callback: symbol(
                    module,
                    "WebAuthNPluginRegisterStatusChangeCallback",
                )?,
                unregister_status_change_callback: symbol(
                    module,
                    "WebAuthNPluginUnregisterStatusChangeCallback",
                )?,
            })
        };
        if result.is_err() {
            unsafe { FreeLibrary(module) };
        }
        result
    }

    pub fn operation_signing_key(&self) -> Result<PluginBuffer<'_>, ApiError> {
        self.public_key(self.get_operation_signing_public_key)
    }

    pub fn user_verification_key(&self) -> Result<PluginBuffer<'_>, ApiError> {
        self.public_key(self.get_user_verification_public_key)
    }

    fn public_key(
        &self,
        function: WebAuthNPluginGetOperationSigningPublicKey,
    ) -> Result<PluginBuffer<'_>, ApiError> {
        let mut len = 0;
        let mut pointer = ptr::null_mut();
        let result = unsafe { function(&CLSID_KEELESS_PASSKEY_WINDOWS, &mut len, &mut pointer) };
        check(result)?;
        PluginBuffer::new(self, pointer, len, self.free_public_key_response)
    }

    pub fn decode_make(
        &self,
        request: &[u8],
    ) -> Result<DecodedMakeCredentialRequest<'_>, ApiError> {
        let mut decoded = ptr::null_mut();
        let result = unsafe {
            (self.decode_make_credential_request)(
                request
                    .len()
                    .try_into()
                    .map_err(|_| ApiError::InvalidBuffer)?,
                request.as_ptr(),
                &mut decoded,
            )
        };
        check(result)?;
        if decoded.is_null() {
            return Err(ApiError::InvalidBuffer);
        }
        Ok(DecodedMakeCredentialRequest { api: self, decoded })
    }

    pub fn decode_assertion(
        &self,
        request: &[u8],
    ) -> Result<DecodedGetAssertionRequest<'_>, ApiError> {
        let mut decoded = ptr::null_mut();
        let result = unsafe {
            (self.decode_get_assertion_request)(
                request
                    .len()
                    .try_into()
                    .map_err(|_| ApiError::InvalidBuffer)?,
                request.as_ptr(),
                &mut decoded,
            )
        };
        check(result)?;
        if decoded.is_null() {
            return Err(ApiError::InvalidBuffer);
        }
        Ok(DecodedGetAssertionRequest { api: self, decoded })
    }

    pub fn encode_make(
        &self,
        response: &WebAuthnCredentialAttestation,
    ) -> Result<EncodedResponse, ApiError> {
        let mut len = 0;
        let mut pointer = ptr::null_mut();
        check(unsafe { (self.encode_make_credential_response)(response, &mut len, &mut pointer) })?;
        EncodedResponse::new(pointer, len)
    }

    pub fn encode_assertion(
        &self,
        response: &WebAuthnCtapCborGetAssertionResponse,
    ) -> Result<EncodedResponse, ApiError> {
        let mut len = 0;
        let mut pointer = ptr::null_mut();
        check(unsafe { (self.encode_get_assertion_response)(response, &mut len, &mut pointer) })?;
        EncodedResponse::new(pointer, len)
    }

    pub fn perform_user_verification(
        &self,
        request: &PluginUserVerificationRequest,
    ) -> Result<PluginBuffer<'_>, ApiError> {
        let mut len = 0;
        let mut pointer = ptr::null_mut();
        check(unsafe { (self.perform_user_verification)(request, &mut len, &mut pointer) })?;
        PluginBuffer::new(self, pointer, len, self.free_user_verification_response)
    }
}

#[repr(C)]
struct OsVersionInfoEx {
    size: u32,
    major: u32,
    minor: u32,
    build: u32,
    platform: u32,
    service_pack: [u16; 128],
    service_pack_major: u16,
    service_pack_minor: u16,
    suite_mask: u16,
    product_type: u8,
    reserved: u8,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetVersion(version: *mut OsVersionInfoEx) -> i32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegGetValueW(
        key: *mut c_void,
        sub_key: *const u16,
        value: *const u16,
        flags: u32,
        value_type: *mut u32,
        data: *mut c_void,
        data_size: *mut u32,
    ) -> i32;
}

fn supported_windows_build() -> bool {
    let mut version = OsVersionInfoEx {
        size: std::mem::size_of::<OsVersionInfoEx>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        service_pack: [0; 128],
        service_pack_major: 0,
        service_pack_minor: 0,
        suite_mask: 0,
        product_type: 0,
        reserved: 0,
    };
    if unsafe { RtlGetVersion(&mut version) } != 0 || version.major != 10 {
        return false;
    }
    if !matches!(version.build, 26100 | 26200) {
        return false;
    }

    const RRF_RT_REG_DWORD: u32 = 0x0000_0010;
    let local_machine = 0x8000_0002_usize as *mut c_void;
    let key = wide("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
    let value = wide("UBR");
    let mut update_build_revision = 0_u32;
    let mut size = std::mem::size_of_val(&update_build_revision) as u32;
    (unsafe {
        RegGetValueW(
            local_machine,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            ptr::null_mut(),
            (&mut update_build_revision as *mut u32).cast(),
            &mut size,
        )
    }) == 0
        && size == std::mem::size_of_val(&update_build_revision) as u32
        && update_build_revision >= 6725
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

impl Drop for Api {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.module) };
    }
}

unsafe fn symbol<T: Copy>(module: HMODULE, name: &'static str) -> Result<T, ApiError> {
    let name = CString::new(name).expect("Windows export name has no NUL");
    let symbol = unsafe { GetProcAddress(module, name.as_ptr() as *const u8) }
        .ok_or(ApiError::MissingExport(name))?;
    // All required functions use WINAPI and one pointer-sized function address.
    Ok(unsafe { mem::transmute_copy(&symbol) })
}

fn check(result: HResult) -> Result<(), ApiError> {
    if result >= S_OK {
        Ok(())
    } else {
        Err(ApiError::HResult(result))
    }
}

pub struct PluginBuffer<'a> {
    api: &'a Api,
    pointer: *mut Byte,
    len: Dword,
    free: unsafe extern "system" fn(*mut Byte),
}

impl<'a> PluginBuffer<'a> {
    fn new(
        api: &'a Api,
        pointer: *mut Byte,
        len: Dword,
        free: unsafe extern "system" fn(*mut Byte),
    ) -> Result<Self, ApiError> {
        if pointer.is_null() || len == 0 {
            if !pointer.is_null() {
                unsafe { free(pointer) };
            }
            return Err(ApiError::InvalidBuffer);
        }
        Ok(Self {
            api,
            pointer,
            len,
            free,
        })
    }

    pub fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.pointer, self.len as usize) }
    }
}

impl Drop for PluginBuffer<'_> {
    fn drop(&mut self) {
        let _ = self.api;
        unsafe { (self.free)(self.pointer) };
    }
}

pub struct DecodedMakeCredentialRequest<'a> {
    api: &'a Api,
    decoded: *mut WebAuthnCtapCborMakeCredentialRequest,
}

impl DecodedMakeCredentialRequest<'_> {
    pub fn get(&self) -> &WebAuthnCtapCborMakeCredentialRequest {
        unsafe { &*self.decoded }
    }
}

impl Drop for DecodedMakeCredentialRequest<'_> {
    fn drop(&mut self) {
        unsafe { (self.api.free_decoded_make_credential_request)(self.decoded) };
    }
}

pub struct DecodedGetAssertionRequest<'a> {
    api: &'a Api,
    decoded: *mut WebAuthnCtapCborGetAssertionRequest,
}

impl DecodedGetAssertionRequest<'_> {
    pub fn get(&self) -> &WebAuthnCtapCborGetAssertionRequest {
        unsafe { &*self.decoded }
    }
}

impl Drop for DecodedGetAssertionRequest<'_> {
    fn drop(&mut self) {
        unsafe { (self.api.free_decoded_get_assertion_request)(self.decoded) };
    }
}

/// A successful encoder result is owned by the COM caller after transfer. The
/// plugin ABI uses the COM task allocator for this response buffer.
pub struct EncodedResponse {
    pointer: *mut Byte,
    len: Dword,
}

impl EncodedResponse {
    fn new(pointer: *mut Byte, len: Dword) -> Result<Self, ApiError> {
        if pointer.is_null() || len == 0 {
            return Err(ApiError::InvalidBuffer);
        }
        Ok(Self { pointer, len })
    }

    pub fn into_raw(self) -> (*mut Byte, Dword) {
        let result = (self.pointer, self.len);
        mem::forget(self);
        result
    }

    /// Free an encoder result which could not be transferred across the COM
    /// boundary because cancellation won after encoding completed.
    pub fn discard(self) {
        unsafe { CoTaskMemFree(self.pointer.cast()) };
        mem::forget(self);
    }
}
