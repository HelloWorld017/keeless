//! Browser host bindings and IndexedDB storage for Keeless.

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
fn normalize_path(path: &str) -> Result<String, &'static str> {
    let segments: Vec<_> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.is_empty() {
        return Ok(String::new());
    }
    if segments
        .iter()
        .any(|segment| *segment == "." || *segment == ".." || segment.contains('\0'))
    {
        return Err("path contains an invalid segment");
    }
    Ok(segments.join("/"))
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
fn imported_database_path(file_name: &str) -> String {
    let name = file_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .chars()
        .map(|character| match character {
            '/' | '\\' | '\0' => '_',
            _ => character,
        })
        .collect::<String>();
    let name = if name.is_empty() {
        "database.kdbx"
    } else {
        &name
    };
    format!("databases/{name}")
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
fn numbered_database_path(path: &str, number: u32) -> String {
    if number <= 1 {
        return path.to_owned();
    }
    let (base, extension) = path
        .rsplit_once('.')
        .map_or((path, ""), |(base, extension)| (base, extension));
    if extension.is_empty() {
        format!("{base} ({number})")
    } else {
        format!("{base} ({number}).{extension}")
    }
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
pub use browser::BrowserCore;

#[cfg(test)]
mod tests {
    use super::{imported_database_path, inclusive_range, normalize_path, numbered_database_path};

    #[test]
    fn normalizes_paths_without_allowing_parent_traversal() {
        assert_eq!(
            normalize_path("/databases//vault.kdbx/"),
            Ok("databases/vault.kdbx".into())
        );
        assert!(normalize_path("databases/../vault.kdbx").is_err());
    }

    #[test]
    fn applies_inclusive_ranges_and_clamps_the_end() {
        assert_eq!(inclusive_range(b"abcdef", 1, 3), Ok(b"bcd".to_vec()));
        assert_eq!(inclusive_range(b"abcdef", 4, 99), Ok(b"ef".to_vec()));
        assert_eq!(inclusive_range(b"abcdef", 9, 10), Ok(Vec::new()));
        assert!(inclusive_range(b"abcdef", 3, 2).is_err());
    }

    #[test]
    fn imported_paths_are_stable_and_drop_client_paths() {
        assert_eq!(
            imported_database_path("C:\\fakepath\\vault.kdbx"),
            "databases/vault.kdbx"
        );
        assert_eq!(imported_database_path(""), "databases/database.kdbx");
        assert_eq!(
            numbered_database_path("databases/vault.kdbx", 2),
            "databases/vault (2).kdbx"
        );
    }
}
