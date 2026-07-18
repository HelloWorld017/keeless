//! Browser host bindings and IndexedDB storage for Keeless.

const DATABASE_PATH: &str = "keeless.kdbx";

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
fn database_path(path: &str) -> Result<&'static str, &'static str> {
    let segments: Vec<_> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.as_slice() != [DATABASE_PATH] {
        return Err("IndexedDB path must be keeless.kdbx");
    }
    Ok(DATABASE_PATH)
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
fn inclusive_range(bytes: &[u8], start: u64, end: u64) -> Result<Vec<u8>, &'static str> {
    if start > end {
        return Err("byte range start must not exceed end");
    }
    let start = usize::try_from(start).map_err(|_| "byte range is too large")?;
    if start >= bytes.len() {
        return Ok(Vec::new());
    }
    let end = usize::try_from(end)
        .unwrap_or(usize::MAX)
        .min(bytes.len().saturating_sub(1));
    Ok(bytes[start..=end].to_vec())
}

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
mod clock;
#[cfg(target_arch = "wasm32")]
mod config;
#[cfg(target_arch = "wasm32")]
mod storages;
#[cfg(target_arch = "wasm32")]
mod utils;

#[cfg(target_arch = "wasm32")]
pub use browser::BrowserCore;

#[cfg(test)]
mod tests {
    use super::{DATABASE_PATH, database_path, inclusive_range};

    #[test]
    fn accepts_only_the_fixed_database_path() {
        assert_eq!(database_path(DATABASE_PATH), Ok(DATABASE_PATH));
        assert_eq!(database_path("/keeless.kdbx/"), Ok(DATABASE_PATH));
        assert!(database_path("databases/keeless.kdbx").is_err());
        assert!(database_path("other.kdbx").is_err());
        assert!(database_path("").is_err());
    }

    #[test]
    fn applies_inclusive_ranges_and_clamps_the_end() {
        assert_eq!(inclusive_range(b"abcdef", 1, 3), Ok(b"bcd".to_vec()));
        assert_eq!(inclusive_range(b"abcdef", 4, 99), Ok(b"ef".to_vec()));
        assert_eq!(inclusive_range(b"abcdef", 9, 10), Ok(Vec::new()));
        assert!(inclusive_range(b"abcdef", 3, 2).is_err());
    }
}
