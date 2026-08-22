//! Reads the package identity assigned to this executable by the external MSIX.

const APPMODEL_ERROR_NO_PACKAGE: u32 = 15_700;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

#[derive(Debug, PartialEq, Eq)]
pub enum PackageIdentity {
    Packaged(String),
    Unpackaged,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentPackageFullName(
        package_full_name_length: *mut u32,
        package_full_name: *mut u16,
    ) -> u32;
}

pub fn current_package_full_name() -> Result<PackageIdentity, String> {
    let mut length = 0;
    match initial_result(
        unsafe { GetCurrentPackageFullName(&mut length, std::ptr::null_mut()) },
        length,
    )? {
        None => Ok(PackageIdentity::Unpackaged),
        Some(length) => {
            let mut name = vec![0_u16; length];
            let mut returned_length = length as u32;
            let result =
                unsafe { GetCurrentPackageFullName(&mut returned_length, name.as_mut_ptr()) };
            if result != 0 {
                return Err(format!("GetCurrentPackageFullName failed with {result:#x}"));
            }
            let name_length = returned_length as usize;
            if name_length == 0 || name_length > name.len() {
                return Err("GetCurrentPackageFullName returned an invalid buffer length".into());
            }
            let name = String::from_utf16(&name[..name_length - 1])
                .map_err(|error| format!("package full name is not valid UTF-16: {error}"))?;
            Ok(PackageIdentity::Packaged(name))
        }
    }
}

fn initial_result(result: u32, length: u32) -> Result<Option<usize>, String> {
    match result {
        APPMODEL_ERROR_NO_PACKAGE => Ok(None),
        ERROR_INSUFFICIENT_BUFFER if length > 0 => Ok(Some(length as usize)),
        ERROR_INSUFFICIENT_BUFFER => {
            Err("GetCurrentPackageFullName returned an empty buffer".into())
        }
        _ => Err(format!("GetCurrentPackageFullName failed with {result:#x}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interprets_package_identity_query_results() {
        assert_eq!(initial_result(APPMODEL_ERROR_NO_PACKAGE, 0), Ok(None));
        assert_eq!(initial_result(ERROR_INSUFFICIENT_BUFFER, 12), Ok(Some(12)));
        assert!(initial_result(ERROR_INSUFFICIENT_BUFFER, 0).is_err());
        assert!(initial_result(5, 0).is_err());
    }
}
