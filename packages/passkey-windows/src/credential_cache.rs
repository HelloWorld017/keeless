//! Windows browser-autofill credential metadata and convergence logic.

use std::collections::HashMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialDetails {
    pub credential_id: Vec<u8>,
    pub rp_id: String,
    pub rp_name: String,
    pub user_id: Vec<u8>,
    pub user_name: String,
    pub user_display_name: String,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct CredentialChanges {
    pub additions: Vec<CredentialDetails>,
    pub removals: Vec<CredentialDetails>,
}

/// Compute an idempotent update from the Windows cache to the database view.
pub fn diff(cached: &[CredentialDetails], desired: &[CredentialDetails]) -> CredentialChanges {
    let cached = cached
        .iter()
        .map(|credential| (credential.credential_id.clone(), credential))
        .collect::<HashMap<_, _>>();
    let desired = desired
        .iter()
        .map(|credential| (credential.credential_id.clone(), credential))
        .collect::<HashMap<_, _>>();
    let mut changes = CredentialChanges::default();

    for (credential_id, cached_credential) in &cached {
        match desired.get(credential_id) {
            None => changes.removals.push((*cached_credential).clone()),
            Some(desired_credential) if *cached_credential != *desired_credential => {
                changes.removals.push((*cached_credential).clone());
                changes.additions.push((*desired_credential).clone());
            }
            Some(_) => {}
        }
    }
    for (credential_id, desired_credential) in &desired {
        if !cached.contains_key(credential_id) {
            changes.additions.push((*desired_credential).clone());
        }
    }
    changes
}

#[cfg(windows)]
pub fn sync(
    api: &crate::api::Api,
    desired: &[CredentialDetails],
) -> Result<(), crate::api::ApiError> {
    let cached = all(api)?;
    let changes = diff(&cached, desired);
    let remove = changes.removals;
    if !remove.is_empty() {
        let ffi = FfiCredentials::new(&remove)?;
        let function = api
            .remove_credentials
            .ok_or(crate::api::ApiError::Unsupported)?;
        check(unsafe {
            function(
                &crate::sdk_bindings::CLSID_KEELESS_PASSKEY_WINDOWS,
                ffi.len(),
                ffi.as_ptr(),
            )
        })?;
    }
    let additions = changes.additions;
    if !additions.is_empty() {
        let ffi = FfiCredentials::new(&additions)?;
        let function = api
            .add_credentials
            .ok_or(crate::api::ApiError::Unsupported)?;
        check(unsafe {
            function(
                &crate::sdk_bindings::CLSID_KEELESS_PASSKEY_WINDOWS,
                ffi.len(),
                ffi.as_ptr(),
            )
        })?;
    }
    Ok(())
}

#[cfg(windows)]
fn all(api: &crate::api::Api) -> Result<Vec<CredentialDetails>, crate::api::ApiError> {
    let get = api
        .get_all_credentials
        .ok_or(crate::api::ApiError::Unsupported)?;
    let free = api
        .free_credential_details_array
        .ok_or(crate::api::ApiError::Unsupported)?;
    let mut count = 0;
    let mut pointer = std::ptr::null_mut();
    check(unsafe {
        get(
            &crate::sdk_bindings::CLSID_KEELESS_PASSKEY_WINDOWS,
            &mut count,
            &mut pointer,
        )
    })?;
    if count == 0 {
        if !pointer.is_null() {
            unsafe { free(count, pointer) };
        }
        return Ok(Vec::new());
    }
    if pointer.is_null() || count > 4096 {
        if !pointer.is_null() {
            unsafe { free(count, pointer) };
        }
        return Err(crate::api::ApiError::InvalidBuffer);
    }

    let result = (|| {
        let values = unsafe { std::slice::from_raw_parts(pointer, count as usize) };
        values.iter().map(copy_detail).collect()
    })();
    unsafe { free(count, pointer) };
    result
}

#[cfg(windows)]
fn copy_detail(
    value: &crate::sdk_bindings::WebAuthnPluginCredentialDetails,
) -> Result<CredentialDetails, crate::api::ApiError> {
    Ok(CredentialDetails {
        credential_id: copy_bytes(value.credential_id, value.credential_id_len, 1024)?,
        rp_id: copy_wide(value.rp_id, true)?,
        rp_name: copy_wide(value.rp_name, true)?,
        user_id: copy_bytes(value.user_id, value.user_id_len, 1024)?,
        user_name: copy_wide(value.user_name, false)?,
        user_display_name: copy_wide(value.user_display_name, false)?,
    })
}

#[cfg(windows)]
struct FfiCredentials {
    _values: Vec<FfiCredentialDetails>,
    raw: Vec<crate::sdk_bindings::WebAuthnPluginCredentialDetails>,
}

#[cfg(windows)]
impl FfiCredentials {
    fn new(values: &[CredentialDetails]) -> Result<Self, crate::api::ApiError> {
        let values = values
            .iter()
            .map(FfiCredentialDetails::new)
            .collect::<Result<Vec<_>, _>>()?;
        let raw = values.iter().map(|value| value.raw).collect();
        Ok(Self {
            _values: values,
            raw,
        })
    }

    fn len(&self) -> crate::sdk_bindings::Dword {
        self._values
            .len()
            .try_into()
            .expect("credential cache count is bounded")
    }

    fn as_ptr(&self) -> *const crate::sdk_bindings::WebAuthnPluginCredentialDetails {
        self.raw
            .as_ptr()
            .cast::<crate::sdk_bindings::WebAuthnPluginCredentialDetails>()
    }
}

#[cfg(windows)]
#[repr(C)]
struct FfiCredentialDetails {
    credential_id: Vec<u8>,
    rp_id: Vec<u16>,
    rp_name: Vec<u16>,
    user_id: Vec<u8>,
    user_name: Vec<u16>,
    user_display_name: Vec<u16>,
    raw: crate::sdk_bindings::WebAuthnPluginCredentialDetails,
}

#[cfg(windows)]
impl FfiCredentialDetails {
    fn new(value: &CredentialDetails) -> Result<Self, crate::api::ApiError> {
        let credential_id = value.credential_id.clone();
        if credential_id.is_empty() || value.user_id.is_empty() {
            return Err(crate::api::ApiError::InvalidBuffer);
        }
        let rp_id = wide(&value.rp_id)?;
        let rp_name = wide(&value.rp_name)?;
        let user_id = value.user_id.clone();
        let user_name = wide(&value.user_name)?;
        let user_display_name = wide(&value.user_display_name)?;
        let raw = crate::sdk_bindings::WebAuthnPluginCredentialDetails {
            credential_id_len: credential_id
                .len()
                .try_into()
                .map_err(|_| crate::api::ApiError::InvalidBuffer)?,
            credential_id: credential_id.as_ptr(),
            rp_id: rp_id.as_ptr(),
            rp_name: rp_name.as_ptr(),
            user_id_len: user_id
                .len()
                .try_into()
                .map_err(|_| crate::api::ApiError::InvalidBuffer)?,
            user_id: user_id.as_ptr(),
            user_name: user_name.as_ptr(),
            user_display_name: user_display_name.as_ptr(),
        };
        Ok(Self {
            credential_id,
            rp_id,
            rp_name,
            user_id,
            user_name,
            user_display_name,
            raw,
        })
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Result<Vec<u16>, crate::api::ApiError> {
    if value.contains('\0') {
        return Err(crate::api::ApiError::InvalidBuffer);
    }
    Ok(value.encode_utf16().chain(Some(0)).collect())
}

#[cfg(windows)]
fn copy_bytes(
    pointer: *const u8,
    len: crate::sdk_bindings::Dword,
    maximum: usize,
) -> Result<Vec<u8>, crate::api::ApiError> {
    let len = len as usize;
    if len == 0 || len > maximum || pointer.is_null() {
        return Err(crate::api::ApiError::InvalidBuffer);
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer, len) }.to_vec())
}

#[cfg(windows)]
fn copy_wide(pointer: *const u16, required: bool) -> Result<String, crate::api::ApiError> {
    if pointer.is_null() {
        return if required {
            Err(crate::api::ApiError::InvalidBuffer)
        } else {
            Ok(String::new())
        };
    }
    for length in 0..=4096 {
        if unsafe { *pointer.add(length) } == 0 {
            let value = String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, length) })
                .map_err(|_| crate::api::ApiError::InvalidBuffer);
            return value.and_then(|value| {
                if required && value.is_empty() {
                    Err(crate::api::ApiError::InvalidBuffer)
                } else {
                    Ok(value)
                }
            });
        }
    }
    Err(crate::api::ApiError::InvalidBuffer)
}

#[cfg(windows)]
fn check(result: crate::sdk_bindings::HResult) -> Result<(), crate::api::ApiError> {
    if result >= crate::sdk_bindings::S_OK {
        Ok(())
    } else {
        Err(crate::api::ApiError::HResult(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential(id: u8, name: &str) -> CredentialDetails {
        CredentialDetails {
            credential_id: vec![id],
            rp_id: "example.com".into(),
            rp_name: "Example".into(),
            user_id: vec![id],
            user_name: name.into(),
            user_display_name: name.into(),
        }
    }

    #[test]
    fn diff_converges_empty_cache_to_database() {
        let changes = diff(&[], &[credential(1, "alice"), credential(2, "bob")]);
        assert_eq!(changes.removals, []);
        assert_eq!(changes.additions.len(), 2);
    }

    #[test]
    fn diff_removes_stale_and_replaces_changed_metadata() {
        let changes = diff(
            &[credential(1, "old"), credential(2, "stale")],
            &[credential(1, "new")],
        );
        assert_eq!(changes.removals.len(), 2);
        assert_eq!(changes.additions, [credential(1, "new")]);
    }

    #[test]
    fn diff_does_nothing_for_identical_metadata() {
        let value = credential(1, "alice");
        assert_eq!(
            diff(std::slice::from_ref(&value), std::slice::from_ref(&value)),
            CredentialChanges::default()
        );
    }
}
