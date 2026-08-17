//! COM callback adapter for the Windows WebAuthn plugin authenticator.

use std::{
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use keeless_schema::{OperationSuccess, RegisterPasskeyResult};

use crate::{
    cancellation::{CeremonyGuard, TransactionId},
    ceremony::{
        AssertionOperation, GetAssertionRequest, MakeCredentialRequest, get_assertion_operation,
        make_credential_operation,
    },
    com::{ComAuthenticator, Provider},
    error::{
        E_FAIL, E_POINTER, ERROR_BUSY, HResult, NTE_BAD_SIGNATURE, NTE_INVALID_PARAMETER,
        NTE_NOT_SUPPORTED, NTE_USER_CANCELLED, request_error,
    },
    sdk_bindings::*,
    verify,
};

const MAX_SIGNATURE_LENGTH: usize = 16 * 1024;
const MAX_STRING_LENGTH: usize = 4096;

struct OperationInput {
    hwnd: Hwnd,
    transaction_id: Guid,
    signature: Vec<u8>,
    encoded_request: Vec<u8>,
}

pub unsafe extern "system" fn make_credential(
    this: *mut c_void,
    request: *const PluginOperationRequest,
    response: *mut PluginOperationResponse,
) -> HResult {
    boundary("make_credential", || {
        if response.is_null() {
            return Err(E_POINTER);
        }
        unsafe {
            (*response).encoded_response_len = 0;
            (*response).encoded_response = ptr::null_mut();
        }
        let provider = unsafe { provider(this)? };
        let input = unsafe { copy_operation_request(request)? };
        println!("keeless-passkey-windows: make_credential copied request");
        verify_operation(provider, &input)?;
        println!("keeless-passkey-windows: make_credential verified operation signature");
        let guard = provider
            .active
            .begin(
                transaction_id(&input.transaction_id),
                input.encoded_request.clone(),
            )
            .map_err(|_| ERROR_BUSY)?;
        println!("keeless-passkey-windows: make_credential started ceremony");
        let decoded = provider
            .api
            .decode_make(&input.encoded_request)
            .map_err(|error| error.hresult())?;
        println!("keeless-passkey-windows: make_credential decoded request");
        let operation = unsafe { make_request(decoded.get())? };
        let operation = make_credential_operation(operation).map_err(request_error)?;
        println!("keeless-passkey-windows: make_credential translated request");
        verify_user(provider, &guard, &input, &rp_id_from_make(decoded.get())?)?;
        println!("keeless-passkey-windows: make_credential verified user");
        if guard.is_cancelled() {
            return Err(NTE_USER_CANCELLED);
        }
        println!("keeless-passkey-windows: make_credential requesting desktop host");
        let success = provider.request(&guard, operation)?;
        let OperationSuccess::RegisterPasskey(result) = success else {
            return Err(E_FAIL);
        };
        println!("keeless-passkey-windows: make_credential received desktop result");
        let encoded = encode_make(provider, result)?;
        println!("keeless-passkey-windows: make_credential encoded response");
        transfer_response(&guard, response, encoded)
    })
}

pub unsafe extern "system" fn get_assertion(
    this: *mut c_void,
    request: *const PluginOperationRequest,
    response: *mut PluginOperationResponse,
) -> HResult {
    boundary("get_assertion", || {
        if response.is_null() {
            return Err(E_POINTER);
        }
        unsafe {
            (*response).encoded_response_len = 0;
            (*response).encoded_response = ptr::null_mut();
        }
        let provider = unsafe { provider(this)? };
        let input = unsafe { copy_operation_request(request)? };
        println!("keeless-passkey-windows: get_assertion copied request");
        verify_operation(provider, &input)?;
        println!("keeless-passkey-windows: get_assertion verified operation signature");
        let guard = provider
            .active
            .begin(
                transaction_id(&input.transaction_id),
                input.encoded_request.clone(),
            )
            .map_err(|_| ERROR_BUSY)?;
        println!("keeless-passkey-windows: get_assertion started ceremony");
        let decoded = provider
            .api
            .decode_assertion(&input.encoded_request)
            .map_err(|error| error.hresult())?;
        println!("keeless-passkey-windows: get_assertion decoded request");
        let request = unsafe { assertion_request(decoded.get())? };
        let rp_id = request.rp_id.clone();
        let AssertionOperation::Interactive(operation) =
            get_assertion_operation(request).map_err(request_error)?
        else {
            return Err(crate::error::NTE_NOT_FOUND);
        };
        println!("keeless-passkey-windows: get_assertion translated request");
        verify_user(provider, &guard, &input, &rp_id)?;
        println!("keeless-passkey-windows: get_assertion verified user");
        if guard.is_cancelled() {
            return Err(NTE_USER_CANCELLED);
        }
        println!("keeless-passkey-windows: get_assertion requesting desktop host");
        let success = provider.request(&guard, *operation)?;
        let OperationSuccess::AssertPasskey(result) = success else {
            return Err(E_FAIL);
        };
        println!("keeless-passkey-windows: get_assertion received desktop result");
        let encoded = encode_assertion(provider, result)?;
        println!("keeless-passkey-windows: get_assertion encoded response");
        transfer_response(&guard, response, encoded)
    })
}

pub unsafe extern "system" fn cancel_operation(
    this: *mut c_void,
    request: *const PluginCancelOperationRequest,
) -> HResult {
    boundary("cancel_operation", || {
        let provider = unsafe { provider(this)? };
        let request = unsafe { copy_cancel_request(request)? };
        println!("keeless-passkey-windows: cancel_operation copied request");
        let Some(target) = provider
            .active
            .cancellation_target(transaction_id(&request.transaction_id))
        else {
            println!("keeless-passkey-windows: cancel_operation has no active ceremony");
            return Ok(());
        };
        let public_key = provider
            .api
            .operation_signing_key()
            .map_err(|error| error.hresult())?;
        verify::verify_signature(
            public_key.bytes(),
            target.encoded_request(),
            &request.signature,
        )
        .map_err(|_| NTE_BAD_SIGNATURE)?;
        let _ = target.cancel();
        println!("keeless-passkey-windows: cancel_operation cancelled ceremony");
        Ok(())
    })
}

pub unsafe extern "system" fn get_lock_status(this: *mut c_void, status: *mut Long) -> HResult {
    boundary("get_lock_status", || {
        if status.is_null() {
            return Err(E_POINTER);
        }
        let provider = unsafe { provider(this)? };
        unsafe {
            *status = provider.lock_status();
        }
        println!(
            "keeless-passkey-windows: get_lock_status returned {}",
            unsafe { *status }
        );
        Ok(())
    })
}

fn verify_operation(provider: &Provider, input: &OperationInput) -> Result<(), HResult> {
    let public_key = provider
        .api
        .operation_signing_key()
        .map_err(|error| error.hresult())?;
    verify::verify_signature(public_key.bytes(), &input.encoded_request, &input.signature)
        .map_err(|_| NTE_BAD_SIGNATURE)
}

fn verify_user(
    provider: &Provider,
    guard: &CeremonyGuard<'_>,
    input: &OperationInput,
    rp_id: &str,
) -> Result<(), HResult> {
    if guard.is_cancelled() {
        return Err(NTE_USER_CANCELLED);
    }
    println!("keeless-passkey-windows: user verification requesting Windows Hello");
    let display_hint = wide(&format!("Use Keeless for {rp_id}"));
    let request = PluginUserVerificationRequest {
        hwnd: input.hwnd,
        transaction_id: &input.transaction_id,
        username: ptr::null(),
        display_hint: display_hint.as_ptr(),
    };
    let signature = provider
        .api
        .perform_user_verification(&request)
        .map_err(|error| error.hresult())?;
    println!("keeless-passkey-windows: user verification received Windows Hello result");
    if guard.is_cancelled() {
        return Err(NTE_USER_CANCELLED);
    }
    let public_key = provider
        .api
        .user_verification_key()
        .map_err(|error| error.hresult())?;
    verify::verify_signature(
        public_key.bytes(),
        &input.encoded_request,
        signature.bytes(),
    )
    .map_err(|_| NTE_BAD_SIGNATURE)?;
    println!("keeless-passkey-windows: user verification signature verified");
    Ok(())
}

fn encode_make(
    provider: &Provider,
    result: RegisterPasskeyResult,
) -> Result<crate::api::EncodedResponse, HResult> {
    let mut authenticator_data = decode_base64(&result.authenticator_data)?;
    let mut credential_id = decode_base64(&result.credential_id)?;
    let none = wide("none");
    let response = WebAuthnCredentialAttestation {
        version: 3,
        format_type: none.as_ptr(),
        authenticator_data_len: byte_len(&authenticator_data)?,
        authenticator_data: authenticator_data.as_mut_ptr(),
        attestation_len: 0,
        attestation: ptr::null_mut(),
        attestation_decode_type: 0,
        attestation_decode: ptr::null_mut(),
        attestation_object_len: 0,
        attestation_object: ptr::null_mut(),
        credential_id_len: byte_len(&credential_id)?,
        credential_id: credential_id.as_mut_ptr(),
        extensions: WebAuthnExtensions {
            count: 0,
            extensions: ptr::null_mut(),
        },
        used_transport: WEBAUTHN_CTAP_TRANSPORT_INTERNAL,
        enterprise_attestation: 0,
        large_blob_supported: 0,
        resident_key: 1,
        prf_enabled: 0,
        unsigned_extension_outputs_len: 0,
        unsigned_extension_outputs: ptr::null_mut(),
        hmac_secret: ptr::null_mut(),
        third_party_payment: 0,
        transports: 0,
        client_data_json_len: 0,
        client_data_json: ptr::null_mut(),
        registration_response_json_len: 0,
        registration_response_json: ptr::null_mut(),
    };
    provider
        .api
        .encode_make(&response)
        .map_err(|error| error.hresult())
}

fn encode_assertion(
    provider: &Provider,
    result: keeless_schema::AssertPasskeyResult,
) -> Result<crate::api::EncodedResponse, HResult> {
    let mut authenticator_data = decode_base64(&result.authenticator_data)?;
    let mut signature = decode_base64(&result.signature)?;
    let mut credential_id = decode_base64(&result.credential_id)?;
    let mut user_handle = decode_base64(&result.user_handle)?;
    let user_name = wide(result.user_name.as_deref().unwrap_or(""));
    let credential_type = wide("public-key");
    let user = WebAuthnUserEntityInformation {
        version: 1,
        id_len: byte_len(&user_handle)?,
        id: user_handle.as_mut_ptr(),
        name: user_name.as_ptr(),
        icon: ptr::null(),
        display_name: ptr::null(),
    };
    let assertion = WebAuthnAssertion {
        version: 4,
        authenticator_data_len: byte_len(&authenticator_data)?,
        authenticator_data: authenticator_data.as_mut_ptr(),
        signature_len: byte_len(&signature)?,
        signature: signature.as_mut_ptr(),
        credential: WebAuthnCredential {
            version: 1,
            id_len: byte_len(&credential_id)?,
            id: credential_id.as_mut_ptr(),
            credential_type: credential_type.as_ptr(),
        },
        user_id_len: byte_len(&user_handle)?,
        user_id: user_handle.as_mut_ptr(),
        extensions: WebAuthnExtensions {
            count: 0,
            extensions: ptr::null_mut(),
        },
        credential_large_blob_len: 0,
        credential_large_blob: ptr::null_mut(),
        credential_large_blob_status: 0,
        hmac_secret: ptr::null_mut(),
        used_transport: WEBAUTHN_CTAP_TRANSPORT_INTERNAL,
        unsigned_extension_outputs_len: 0,
        unsigned_extension_outputs: ptr::null_mut(),
        client_data_json_len: 0,
        client_data_json: ptr::null_mut(),
        authentication_response_json_len: 0,
        authentication_response_json: ptr::null_mut(),
    };
    let response = WebAuthnCtapCborGetAssertionResponse {
        assertion,
        user_information: &user,
        number_of_credentials: 1,
        user_selected: i32::from(result.user_selected),
        large_blob_key_len: 0,
        large_blob_key: ptr::null_mut(),
        unsigned_extension_outputs_len: 0,
        unsigned_extension_outputs: ptr::null_mut(),
    };
    provider
        .api
        .encode_assertion(&response)
        .map_err(|error| error.hresult())
}

fn transfer_response(
    guard: &CeremonyGuard<'_>,
    response: *mut PluginOperationResponse,
    encoded: crate::api::EncodedResponse,
) -> Result<(), HResult> {
    if !guard.complete() {
        encoded.discard();
        return Err(NTE_USER_CANCELLED);
    }
    let (bytes, len) = encoded.into_raw();
    unsafe {
        (*response).encoded_response_len = len;
        (*response).encoded_response = bytes;
    }
    Ok(())
}

unsafe fn make_request(
    request: &WebAuthnCtapCborMakeCredentialRequest,
) -> Result<MakeCredentialRequest, HResult> {
    reject_make_extensions(request)?;
    let rp = required(unsafe { request.rp_information.as_ref() })?;
    let user = required(unsafe { request.user_information.as_ref() })?;
    let rp_id = unsafe { utf8(request.rp_id, request.rp_id_len)? };
    let rp_name = unsafe { optional_wide(rp.name)? };
    let user_name = unsafe { required_wide(user.name)? };
    let user_handle = unsafe { bytes(user.id, user.id_len, MAX_STRING_LENGTH)? };
    let client_data_hash =
        unsafe { bytes(request.client_data_hash, request.client_data_hash_len, 64)? };
    let algorithms = unsafe { algorithms(&request.credential_parameters)? };
    let exclude_credential_ids = unsafe { credential_ids(&request.credential_list)? };
    Ok(MakeCredentialRequest {
        rp_id,
        rp_name,
        user_name,
        user_handle,
        client_data_hash,
        algorithms,
        exclude_credential_ids,
    })
}

unsafe fn assertion_request(
    request: &WebAuthnCtapCborGetAssertionRequest,
) -> Result<GetAssertionRequest, HResult> {
    reject_assertion_extensions(request)?;
    let user_presence = match unsafe { request.authenticator_options.as_ref() } {
        Some(options) => {
            validate_option(options.user_presence)?;
            validate_option(options.user_verification)?;
            validate_option(options.require_resident_key)?;
            options.user_presence >= 0
        }
        None => true,
    };
    Ok(GetAssertionRequest {
        rp_id: unsafe { utf8(request.raw_rp_id, request.raw_rp_id_len)? },
        client_data_hash: unsafe {
            bytes(request.client_data_hash, request.client_data_hash_len, 64)?
        },
        allow_credential_ids: unsafe { credential_ids(&request.credential_list)? },
        user_presence,
    })
}

fn rp_id_from_make(request: &WebAuthnCtapCborMakeCredentialRequest) -> Result<String, HResult> {
    unsafe { utf8(request.rp_id, request.rp_id_len) }
}

fn reject_make_extensions(request: &WebAuthnCtapCborMakeCredentialRequest) -> Result<(), HResult> {
    if request.cbor_extensions_map_len != 0
        || request.empty_pin_auth != 0
        || request.pin_auth_len != 0
        || request.hmac_secret_extension != 0
        || !request.hmac_secret_make_credential_extension.is_null()
        || request.prf_extension != 0
        || request.hmac_secret_salt_values_len != 0
        || request.credential_protection != 0
        || request.pin_protocol != 0
        || request.enterprise_attestation != 0
        || request.credential_blob_extension_len != 0
        || request.large_blob_key_extension != 0
        || request.large_blob_support != 0
        || request.minimum_pin_length_extension != 0
        || request.json_extension_len != 0
    {
        return Err(NTE_NOT_SUPPORTED);
    }
    if let Some(options) = unsafe { request.authenticator_options.as_ref() } {
        validate_option(options.user_presence)?;
        validate_option(options.user_verification)?;
        validate_option(options.require_resident_key)?;
        if options.user_presence < 0 {
            return Err(NTE_NOT_SUPPORTED);
        }
    }
    Ok(())
}

fn reject_assertion_extensions(
    request: &WebAuthnCtapCborGetAssertionRequest,
) -> Result<(), HResult> {
    if request.cbor_extensions_map_len != 0
        || request.empty_pin_auth != 0
        || request.pin_auth_len != 0
        || !request.hmac_secret_salt_extension.is_null()
        || request.hmac_secret_salt_values_len != 0
        || request.pin_protocol != 0
        || request.credential_blob_extension != 0
        || request.large_blob_key_extension != 0
        || request.credential_large_blob_operation != 0
        || request.credential_large_blob_compressed_len != 0
        || request.credential_large_blob_original_size != 0
        || request.json_extension_len != 0
    {
        return Err(NTE_NOT_SUPPORTED);
    }
    Ok(())
}

fn validate_option(value: Long) -> Result<(), HResult> {
    (-1..=1)
        .contains(&value)
        .then_some(())
        .ok_or(NTE_INVALID_PARAMETER)
}

unsafe fn algorithms(parameters: &WebAuthnCoseCredentialParameters) -> Result<Vec<i32>, HResult> {
    let count = parameters.count as usize;
    if count > 32 {
        return Err(NTE_INVALID_PARAMETER);
    }
    let parameters = unsafe { slice(parameters.parameters, count)? };
    let mut algorithms = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        if unsafe { optional_wide(parameter.credential_type)? }.as_deref() == Some("public-key") {
            algorithms.push(parameter.algorithm);
        }
    }
    Ok(algorithms)
}

unsafe fn credential_ids(list: &WebAuthnCredentialList) -> Result<Vec<Vec<u8>>, HResult> {
    let count = list.count as usize;
    if count > keeless_passkey_ctap::response::MAX_CREDENTIAL_COUNT_IN_LIST as usize {
        return Err(NTE_INVALID_PARAMETER);
    }
    let credentials = unsafe { slice(list.credentials, count)? };
    credentials
        .iter()
        .map(|credential| {
            let credential = required(unsafe { credential.as_ref() })?;
            unsafe {
                bytes(
                    credential.id,
                    credential.id_len,
                    keeless_passkey_ctap::response::MAX_CREDENTIAL_ID_LENGTH as usize,
                )
            }
        })
        .collect()
}

unsafe fn copy_operation_request(
    request: *const PluginOperationRequest,
) -> Result<OperationInput, HResult> {
    let request = required(unsafe { request.as_ref() })?;
    if request.request_type != WEBAUTHN_PLUGIN_REQUEST_TYPE_CTAP2_CBOR {
        return Err(NTE_NOT_SUPPORTED);
    }
    Ok(OperationInput {
        hwnd: request.hwnd,
        transaction_id: request.transaction_id,
        signature: unsafe {
            bytes(
                request.request_signature,
                request.request_signature_len,
                MAX_SIGNATURE_LENGTH,
            )?
        },
        encoded_request: unsafe {
            bytes(
                request.encoded_request,
                request.encoded_request_len,
                keeless_passkey_ctap::response::MAX_MESSAGE_SIZE as usize,
            )?
        },
    })
}

struct CancelInput {
    transaction_id: Guid,
    signature: Vec<u8>,
}

unsafe fn copy_cancel_request(
    request: *const PluginCancelOperationRequest,
) -> Result<CancelInput, HResult> {
    let request = required(unsafe { request.as_ref() })?;
    Ok(CancelInput {
        transaction_id: request.transaction_id,
        signature: unsafe {
            bytes(
                request.request_signature,
                request.request_signature_len,
                MAX_SIGNATURE_LENGTH,
            )?
        },
    })
}

fn required<T>(value: Option<&T>) -> Result<&T, HResult> {
    value.ok_or(NTE_INVALID_PARAMETER)
}

unsafe fn bytes(pointer: *const u8, len: Dword, maximum: usize) -> Result<Vec<u8>, HResult> {
    let len = len as usize;
    if len == 0 || len > maximum || pointer.is_null() {
        return Err(NTE_INVALID_PARAMETER);
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer, len) }.to_vec())
}

unsafe fn utf8(pointer: *const u8, len: Dword) -> Result<String, HResult> {
    String::from_utf8(unsafe { bytes(pointer, len, MAX_STRING_LENGTH)? })
        .map_err(|_| NTE_INVALID_PARAMETER)
}

unsafe fn required_wide(pointer: *const u16) -> Result<String, HResult> {
    unsafe { optional_wide(pointer)? }
        .filter(|value| !value.is_empty())
        .ok_or(NTE_INVALID_PARAMETER)
}

unsafe fn optional_wide(pointer: *const u16) -> Result<Option<String>, HResult> {
    if pointer.is_null() {
        return Ok(None);
    }
    let mut len = 0;
    while len < MAX_STRING_LENGTH {
        if unsafe { *pointer.add(len) } == 0 {
            let value = unsafe { std::slice::from_raw_parts(pointer, len) };
            return String::from_utf16(value)
                .map(Some)
                .map_err(|_| NTE_INVALID_PARAMETER);
        }
        len += 1;
    }
    Err(NTE_INVALID_PARAMETER)
}

unsafe fn slice<'a, T>(pointer: *const T, len: usize) -> Result<&'a [T], HResult> {
    if len == 0 {
        return Ok(&[]);
    }
    if pointer.is_null() {
        return Err(NTE_INVALID_PARAMETER);
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer, len) })
}

fn decode_base64(value: &str) -> Result<Vec<u8>, HResult> {
    let value = URL_SAFE_NO_PAD.decode(value).map_err(|_| E_FAIL)?;
    (!value.is_empty()).then_some(value).ok_or(E_FAIL)
}

fn byte_len(bytes: &[u8]) -> Result<Dword, HResult> {
    bytes.len().try_into().map_err(|_| E_FAIL)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn transaction_id(guid: &Guid) -> TransactionId {
    let mut transaction_id = [0_u8; 16];
    transaction_id[..4].copy_from_slice(&guid.data1.to_le_bytes());
    transaction_id[4..6].copy_from_slice(&guid.data2.to_le_bytes());
    transaction_id[6..8].copy_from_slice(&guid.data3.to_le_bytes());
    transaction_id[8..].copy_from_slice(&guid.data4);
    transaction_id
}

unsafe fn provider(this: *mut c_void) -> Result<&'static Provider, HResult> {
    let authenticator = unsafe { (this as *mut ComAuthenticator).as_ref() }.ok_or(E_POINTER)?;
    Ok(&authenticator.provider)
}

fn boundary(name: &str, callback: impl FnOnce() -> Result<(), HResult>) -> HResult {
    println!("keeless-passkey-windows: {name} entered");
    match catch_unwind(AssertUnwindSafe(callback)) {
        Ok(Ok(())) => {
            println!("keeless-passkey-windows: {name} completed with 0x00000000");
            S_OK
        }
        Ok(Err(error)) => {
            println!("keeless-passkey-windows: {name} failed with {error:#010x}");
            error
        }
        Err(_) => {
            println!("keeless-passkey-windows: {name} panicked");
            E_FAIL
        }
    }
}
