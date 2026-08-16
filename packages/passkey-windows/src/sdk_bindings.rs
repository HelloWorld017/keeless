//! Minimal bindings generated from the public Microsoft WebAuthn plugin ABI.
//!
//! Source: `microsoft/webauthn` commit
//! `ef82c157125a0490e05f6ea82a7adb1b8e1bad08`, files
//! `pluginauthenticator.idl`, `pluginauthenticator.h`, `webauthnplugin.h`, and
//! `webauthn.h`. Those inputs are MIT licensed by Microsoft Corporation.
//!
//! This checked-in subset deliberately contains only the ABI used by the
//! cache-free Keeless provider. It is not linked to `webauthn.dll`; `api.rs`
//! resolves every function at runtime so unsupported Windows versions can load
//! the executable and report an unsupported result.

use std::ffi::c_void;

pub type Bool = i32;
pub type Byte = u8;
pub type Dword = u32;
pub type HResult = i32;
pub type Hwnd = isize;
pub type Long = i32;

pub const S_OK: HResult = 0;
pub const WEBAUTHN_PLUGIN_REQUEST_TYPE_CTAP2_CBOR: Dword = 1;
pub const PLUGIN_LOCKED: Long = 0;
pub const PLUGIN_UNLOCKED: Long = 1;
pub const AUTHENTICATOR_STATE_DISABLED: Dword = 0;
pub const AUTHENTICATOR_STATE_ENABLED: Dword = 1;
pub const WEBAUTHN_CTAP_TRANSPORT_INTERNAL: Dword = 0x10;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

pub const CLSID_KEELESS_PASSKEY_WINDOWS: Guid = Guid {
    data1: 0x13ab_eff0,
    data2: 0x71c5,
    data3: 0x49e3,
    data4: [0x9f, 0x2f, 0xc2, 0x07, 0xa2, 0x8c, 0xdb, 0x9d],
};

pub const IID_IUNKNOWN: Guid = Guid {
    data1: 0,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
pub const IID_ICLASS_FACTORY: Guid = Guid {
    data1: 1,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
pub const IID_IPLUGIN_AUTHENTICATOR: Guid = Guid {
    data1: 0xd26b_cf6f,
    data2: 0xb54c,
    data3: 0x43ff,
    data4: [0x9f, 0x06, 0xd5, 0xbf, 0x14, 0x86, 0x25, 0xf7],
};

#[repr(C)]
pub struct PluginOperationRequest {
    pub hwnd: Hwnd,
    pub transaction_id: Guid,
    pub request_signature_len: Dword,
    pub request_signature: *const Byte,
    pub request_type: Dword,
    pub encoded_request_len: Dword,
    pub encoded_request: *const Byte,
}

#[repr(C)]
pub struct PluginOperationResponse {
    pub encoded_response_len: Dword,
    pub encoded_response: *mut Byte,
}

#[repr(C)]
pub struct PluginCancelOperationRequest {
    pub transaction_id: Guid,
    pub request_signature_len: Dword,
    pub request_signature: *const Byte,
}

#[repr(C)]
pub struct PluginAddAuthenticatorOptions {
    pub authenticator_name: *const u16,
    pub clsid: *const Guid,
    pub plugin_rp_id: *const u16,
    pub light_theme_logo_svg: *const u16,
    pub dark_theme_logo_svg: *const u16,
    pub authenticator_info_len: Dword,
    pub authenticator_info: *const Byte,
    pub supported_rp_ids_count: Dword,
    pub supported_rp_ids: *const *const u16,
}

#[repr(C)]
pub struct PluginAddAuthenticatorResponse {
    pub operation_signing_public_key_len: Dword,
    pub operation_signing_public_key: *mut Byte,
}

#[repr(C)]
pub struct PluginUserVerificationRequest {
    pub hwnd: Hwnd,
    pub transaction_id: *const Guid,
    pub username: *const u16,
    pub display_hint: *const u16,
}

#[repr(C)]
pub struct WebAuthnRpEntityInformation {
    pub version: Dword,
    pub id: *const u16,
    pub name: *const u16,
    pub icon: *const u16,
}

#[repr(C)]
pub struct WebAuthnUserEntityInformation {
    pub version: Dword,
    pub id_len: Dword,
    pub id: *mut Byte,
    pub name: *const u16,
    pub icon: *const u16,
    pub display_name: *const u16,
}

#[repr(C)]
pub struct WebAuthnCoseCredentialParameter {
    pub version: Dword,
    pub credential_type: *const u16,
    pub algorithm: Long,
}

#[repr(C)]
pub struct WebAuthnCoseCredentialParameters {
    pub count: Dword,
    pub parameters: *mut WebAuthnCoseCredentialParameter,
}

#[repr(C)]
pub struct WebAuthnCredentialEx {
    pub version: Dword,
    pub id_len: Dword,
    pub id: *mut Byte,
    pub credential_type: *const u16,
    pub transports: Dword,
}

#[repr(C)]
pub struct WebAuthnCredentialList {
    pub count: Dword,
    pub credentials: *mut *mut WebAuthnCredentialEx,
}

#[repr(C)]
pub struct WebAuthnCtapCborAuthenticatorOptions {
    pub version: Dword,
    pub user_presence: Long,
    pub user_verification: Long,
    pub require_resident_key: Long,
}

#[repr(C)]
pub struct WebAuthnCtapCborMakeCredentialRequest {
    pub version: Dword,
    pub rp_id_len: Dword,
    pub rp_id: *mut Byte,
    pub client_data_hash_len: Dword,
    pub client_data_hash: *mut Byte,
    pub rp_information: *const WebAuthnRpEntityInformation,
    pub user_information: *const WebAuthnUserEntityInformation,
    pub credential_parameters: WebAuthnCoseCredentialParameters,
    pub credential_list: WebAuthnCredentialList,
    pub cbor_extensions_map_len: Dword,
    pub cbor_extensions_map: *mut Byte,
    pub authenticator_options: *mut WebAuthnCtapCborAuthenticatorOptions,
    pub empty_pin_auth: Bool,
    pub pin_auth_len: Dword,
    pub pin_auth: *mut Byte,
    pub hmac_secret_extension: Long,
    pub hmac_secret_make_credential_extension: *mut c_void,
    pub prf_extension: Long,
    pub hmac_secret_salt_values_len: Dword,
    pub hmac_secret_salt_values: *mut Byte,
    pub credential_protection: Dword,
    pub pin_protocol: Dword,
    pub enterprise_attestation: Dword,
    pub credential_blob_extension_len: Dword,
    pub credential_blob_extension: *mut Byte,
    pub large_blob_key_extension: Long,
    pub large_blob_support: Dword,
    pub minimum_pin_length_extension: Long,
    pub json_extension_len: Dword,
    pub json_extension: *mut Byte,
}

#[repr(C)]
pub struct WebAuthnCtapCborGetAssertionRequest {
    pub version: Dword,
    pub rp_id: *const u16,
    pub raw_rp_id_len: Dword,
    pub raw_rp_id: *mut Byte,
    pub client_data_hash_len: Dword,
    pub client_data_hash: *mut Byte,
    pub credential_list: WebAuthnCredentialList,
    pub cbor_extensions_map_len: Dword,
    pub cbor_extensions_map: *mut Byte,
    pub authenticator_options: *mut WebAuthnCtapCborAuthenticatorOptions,
    pub empty_pin_auth: Bool,
    pub pin_auth_len: Dword,
    pub pin_auth: *mut Byte,
    pub hmac_secret_salt_extension: *mut c_void,
    pub hmac_secret_salt_values_len: Dword,
    pub hmac_secret_salt_values: *mut Byte,
    pub pin_protocol: Dword,
    pub credential_blob_extension: Long,
    pub large_blob_key_extension: Long,
    pub credential_large_blob_operation: Dword,
    pub credential_large_blob_compressed_len: Dword,
    pub credential_large_blob_compressed: *mut Byte,
    pub credential_large_blob_original_size: Dword,
    pub json_extension_len: Dword,
    pub json_extension: *mut Byte,
}

#[repr(C)]
pub struct WebAuthnExtension {
    pub identifier: *const u16,
    pub data_len: Dword,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct WebAuthnExtensions {
    pub count: Dword,
    pub extensions: *mut WebAuthnExtension,
}

#[repr(C)]
pub struct WebAuthnCredentialAttestation {
    pub version: Dword,
    pub format_type: *const u16,
    pub authenticator_data_len: Dword,
    pub authenticator_data: *mut Byte,
    pub attestation_len: Dword,
    pub attestation: *mut Byte,
    pub attestation_decode_type: Dword,
    pub attestation_decode: *mut c_void,
    pub attestation_object_len: Dword,
    pub attestation_object: *mut Byte,
    pub credential_id_len: Dword,
    pub credential_id: *mut Byte,
    pub extensions: WebAuthnExtensions,
    pub used_transport: Dword,
    pub enterprise_attestation: Bool,
    pub large_blob_supported: Bool,
    pub resident_key: Bool,
    pub prf_enabled: Bool,
    pub unsigned_extension_outputs_len: Dword,
    pub unsigned_extension_outputs: *mut Byte,
    pub hmac_secret: *mut c_void,
    pub third_party_payment: Bool,
    pub transports: Dword,
    pub client_data_json_len: Dword,
    pub client_data_json: *mut Byte,
    pub registration_response_json_len: Dword,
    pub registration_response_json: *mut Byte,
}

#[repr(C)]
pub struct WebAuthnCredential {
    pub version: Dword,
    pub id_len: Dword,
    pub id: *mut Byte,
    pub credential_type: *const u16,
}

#[repr(C)]
pub struct WebAuthnAssertion {
    pub version: Dword,
    pub authenticator_data_len: Dword,
    pub authenticator_data: *mut Byte,
    pub signature_len: Dword,
    pub signature: *mut Byte,
    pub credential: WebAuthnCredential,
    pub user_id_len: Dword,
    pub user_id: *mut Byte,
    pub extensions: WebAuthnExtensions,
    pub credential_large_blob_len: Dword,
    pub credential_large_blob: *mut Byte,
    pub credential_large_blob_status: Dword,
    pub hmac_secret: *mut c_void,
    pub used_transport: Dword,
    pub unsigned_extension_outputs_len: Dword,
    pub unsigned_extension_outputs: *mut Byte,
    pub client_data_json_len: Dword,
    pub client_data_json: *mut Byte,
    pub authentication_response_json_len: Dword,
    pub authentication_response_json: *mut Byte,
}

#[repr(C)]
pub struct WebAuthnCtapCborGetAssertionResponse {
    pub assertion: WebAuthnAssertion,
    pub user_information: *const WebAuthnUserEntityInformation,
    pub number_of_credentials: Dword,
    pub user_selected: Long,
    pub large_blob_key_len: Dword,
    pub large_blob_key: *mut Byte,
    pub unsigned_extension_outputs_len: Dword,
    pub unsigned_extension_outputs: *mut Byte,
}

#[repr(C)]
pub struct IPluginAuthenticatorVtbl {
    pub query_interface:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HResult,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub make_credential: unsafe extern "system" fn(
        *mut c_void,
        *const PluginOperationRequest,
        *mut PluginOperationResponse,
    ) -> HResult,
    pub get_assertion: unsafe extern "system" fn(
        *mut c_void,
        *const PluginOperationRequest,
        *mut PluginOperationResponse,
    ) -> HResult,
    pub cancel_operation:
        unsafe extern "system" fn(*mut c_void, *const PluginCancelOperationRequest) -> HResult,
    pub get_lock_status: unsafe extern "system" fn(*mut c_void, *mut Long) -> HResult,
}

#[repr(C)]
pub struct IClassFactoryVtbl {
    pub query_interface:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HResult,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub create_instance: unsafe extern "system" fn(
        *mut c_void,
        *mut c_void,
        *const Guid,
        *mut *mut c_void,
    ) -> HResult,
    pub lock_server: unsafe extern "system" fn(*mut c_void, Bool) -> HResult,
}

pub type WebAuthNPluginGetAuthenticatorState =
    unsafe extern "system" fn(*const Guid, *mut Dword) -> HResult;
pub type WebAuthNPluginAddAuthenticator = unsafe extern "system" fn(
    *const PluginAddAuthenticatorOptions,
    *mut *mut PluginAddAuthenticatorResponse,
) -> HResult;
pub type WebAuthNPluginFreeAddAuthenticatorResponse =
    unsafe extern "system" fn(*mut PluginAddAuthenticatorResponse);
pub type WebAuthNPluginRemoveAuthenticator = unsafe extern "system" fn(*const Guid) -> HResult;
pub type WebAuthNPluginPerformUserVerification = unsafe extern "system" fn(
    *const PluginUserVerificationRequest,
    *mut Dword,
    *mut *mut Byte,
) -> HResult;
pub type WebAuthNPluginFreeUserVerificationResponse = unsafe extern "system" fn(*mut Byte);
pub type WebAuthNPluginGetUserVerificationPublicKey =
    unsafe extern "system" fn(*const Guid, *mut Dword, *mut *mut Byte) -> HResult;
pub type WebAuthNPluginGetOperationSigningPublicKey =
    unsafe extern "system" fn(*const Guid, *mut Dword, *mut *mut Byte) -> HResult;
pub type WebAuthNPluginFreePublicKeyResponse = unsafe extern "system" fn(*mut Byte);
pub type WebAuthNDecodeMakeCredentialRequest = unsafe extern "system" fn(
    Dword,
    *const Byte,
    *mut *mut WebAuthnCtapCborMakeCredentialRequest,
) -> HResult;
pub type WebAuthNFreeDecodedMakeCredentialRequest =
    unsafe extern "system" fn(*mut WebAuthnCtapCborMakeCredentialRequest);
pub type WebAuthNEncodeMakeCredentialResponse = unsafe extern "system" fn(
    *const WebAuthnCredentialAttestation,
    *mut Dword,
    *mut *mut Byte,
) -> HResult;
pub type WebAuthNDecodeGetAssertionRequest = unsafe extern "system" fn(
    Dword,
    *const Byte,
    *mut *mut WebAuthnCtapCborGetAssertionRequest,
) -> HResult;
pub type WebAuthNFreeDecodedGetAssertionRequest =
    unsafe extern "system" fn(*mut WebAuthnCtapCborGetAssertionRequest);
pub type WebAuthNEncodeGetAssertionResponse = unsafe extern "system" fn(
    *const WebAuthnCtapCborGetAssertionResponse,
    *mut Dword,
    *mut *mut Byte,
) -> HResult;
pub type WebAuthNPluginStatusChangeCallback = unsafe extern "system" fn(*mut c_void);
pub type WebAuthNPluginRegisterStatusChangeCallback = unsafe extern "system" fn(
    WebAuthNPluginStatusChangeCallback,
    *mut c_void,
    *const Guid,
    *mut Dword,
) -> HResult;
pub type WebAuthNPluginUnregisterStatusChangeCallback =
    unsafe extern "system" fn(*mut Dword) -> HResult;
