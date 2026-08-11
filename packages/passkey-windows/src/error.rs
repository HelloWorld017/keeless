//! Mapping Core operation failures to the HRESULTs the Windows provider expects.

/// HRESULT values are stored as signed values because COM methods return `HRESULT`.
pub type HResult = i32;

pub const NTE_BAD_SIGNATURE: HResult = 0x8009_0006_u32 as i32;
pub const NTE_EXISTS: HResult = 0x8009_000f_u32 as i32;
pub const NTE_NOT_FOUND: HResult = 0x8009_0011_u32 as i32;
pub const NTE_INVALID_PARAMETER: HResult = 0x8009_0027_u32 as i32;
pub const NTE_NOT_SUPPORTED: HResult = 0x8009_0029_u32 as i32;
pub const NTE_USER_CANCELLED: HResult = 0x8009_0036_u32 as i32;
pub const ERROR_BUSY: HResult = 0x8007_00aa_u32 as i32;
pub const E_FAIL: HResult = 0x8000_4005_u32 as i32;

/// Map a Core operation error code without exposing database or cryptographic
/// detail to the browser.
pub fn core_operation_error(code: &str) -> HResult {
    match code {
        "passkey_excluded" => NTE_EXISTS,
        "passkey_not_found" => NTE_NOT_FOUND,
        "passkey_unsupported_algorithm" => NTE_NOT_SUPPORTED,
        "invalid_passkey_request" => NTE_INVALID_PARAMETER,
        "passkey_consent_denied" | "password_required" | "invalid_credentials" => {
            NTE_USER_CANCELLED
        }
        _ => E_FAIL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_core_failures_to_browser_fallback_results() {
        assert_eq!(core_operation_error("passkey_excluded"), NTE_EXISTS);
        assert_eq!(core_operation_error("passkey_not_found"), NTE_NOT_FOUND);
        assert_eq!(
            core_operation_error("passkey_unsupported_algorithm"),
            NTE_NOT_SUPPORTED
        );
        assert_eq!(
            core_operation_error("passkey_consent_denied"),
            NTE_USER_CANCELLED
        );
        assert_eq!(core_operation_error("unknown"), E_FAIL);
    }
}
