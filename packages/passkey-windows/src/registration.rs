//! Registration lifecycle for the Keeless provider.

use std::ptr;

use keeless_passkey_ctap::{
    KEELESS_AAGUID,
    response::{AuthenticatorInfo, authenticator_info_cbor},
};

use crate::{
    api::{Api, ApiError},
    error::NTE_NOT_FOUND,
    sdk_bindings::*,
};

pub const PROVIDER_NAME: &str = "Keeless";
const PLUGIN_RP_ID: &str = "keeless.nenw.dev";

pub fn enable(api: &Api) -> Result<(), ApiError> {
    let name = wide(PROVIDER_NAME);
    let rp_id = wide(PLUGIN_RP_ID);
    let info = authenticator_info_cbor(&AuthenticatorInfo {
        aaguid: &KEELESS_AAGUID,
        platform_device: true,
        transports: &["internal"],
    })
    .map_err(|_| ApiError::InvalidBuffer)?;
    let options = PluginAddAuthenticatorOptions {
        authenticator_name: name.as_ptr(),
        clsid: &CLSID_KEELESS_PASSKEY_WINDOWS,
        plugin_rp_id: rp_id.as_ptr(),
        light_theme_logo_svg: ptr::null(),
        dark_theme_logo_svg: ptr::null(),
        authenticator_info_len: info.len().try_into().map_err(|_| ApiError::InvalidBuffer)?,
        authenticator_info: info.as_ptr(),
        supported_rp_ids_count: 0,
        supported_rp_ids: ptr::null(),
    };
    let mut response = ptr::null_mut();
    let result = unsafe { (api.add_authenticator)(&options, &mut response) };
    if result < S_OK {
        return Err(ApiError::HResult(result));
    }
    if !response.is_null() {
        unsafe { (api.free_add_authenticator_response)(response) };
    }
    Ok(())
}

pub fn disable(api: &Api) -> Result<(), ApiError> {
    let result = unsafe { (api.remove_authenticator)(&CLSID_KEELESS_PASSKEY_WINDOWS) };
    if result >= S_OK || result == NTE_NOT_FOUND {
        Ok(())
    } else {
        Err(ApiError::HResult(result))
    }
}

pub fn is_enabled(api: &Api) -> Result<bool, ApiError> {
    let mut state = AUTHENTICATOR_STATE_DISABLED;
    let result =
        unsafe { (api.get_authenticator_state)(&CLSID_KEELESS_PASSKEY_WINDOWS, &mut state) };
    if result < S_OK {
        return Err(ApiError::HResult(result));
    }
    Ok(state == AUTHENTICATOR_STATE_ENABLED)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
