//! Browser host bindings and IndexedDB storage for Keeless.

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
fn database_path(path: &str) -> Result<String, &'static str> {
    let segments: Vec<_> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.is_empty() {
        return Ok("keeless.kdbx".into());
    }
    if segments
        .iter()
        .any(|segment| *segment == "." || *segment == "..")
    {
        return Err("IndexedDB path must not contain '.' or '..' segments");
    }
    Ok(segments.join("/"))
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
mod persistence;
#[cfg(target_arch = "wasm32")]
mod storages;
#[cfg(target_arch = "wasm32")]
mod utils;

#[cfg(target_arch = "wasm32")]
pub use browser::BrowserCore;

#[cfg(test)]
mod tests {
    use super::{database_path, inclusive_range};

    #[test]
    fn normalizes_database_paths() {
        assert_eq!(database_path("/keeless.kdbx/"), Ok("keeless.kdbx".into()));
        assert_eq!(
            database_path("databases/vault.kdbx"),
            Ok("databases/vault.kdbx".into())
        );
        assert_eq!(database_path(""), Ok("keeless.kdbx".into()));
        assert!(database_path("../vault.kdbx").is_err());
    }

    #[test]
    fn applies_inclusive_ranges_and_clamps_the_end() {
        assert_eq!(inclusive_range(b"abcdef", 1, 3), Ok(b"bcd".to_vec()));
        assert_eq!(inclusive_range(b"abcdef", 4, 99), Ok(b"ef".to_vec()));
        assert_eq!(inclusive_range(b"abcdef", 9, 10), Ok(Vec::new()));
        assert!(inclusive_range(b"abcdef", 3, 2).is_err());
    }
}
