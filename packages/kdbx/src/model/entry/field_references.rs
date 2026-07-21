//! Field references engine
//!
//! Parses and resolves KeePass field references like {REF:@I:...}

use super::Entry;
use crate::crypto::memory_protection::MemoryField;
use crate::model::db::{CompositeKey, Database};
use crate::model::exception::DatabaseResult;
use zeroize::Zeroizing;

/// Field reference target
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefTarget {
    Title,
    UserName,
    Password,
    Url,
    Notes,
    CustomField,
}

/// A parsed field reference
#[derive(Debug, Clone)]
pub struct FieldReference {
    pub target_field: RefTarget,
    pub search_in: RefTarget,
    pub search_value: String,
}

impl FieldReference {
    /// Parse a KeePass field reference string.
    /// Format: {REF:<target>@<search_in>:<search_value>}
    /// Example: {REF:P@I:1234} means "get Password from entry with ID 1234"
    pub fn parse(reference: &str) -> Option<FieldReference> {
        if !reference.starts_with("{REF:") || !reference.ends_with('}') {
            return None;
        }

        let inner = &reference[5..reference.len() - 1]; // Remove {REF: and }
        let parts: Vec<&str> = inner.splitn(2, '@').collect();
        if parts.len() != 2 {
            return None;
        }

        let target = parse_ref_field(parts[0].chars().next()?)?;

        let search_parts: Vec<&str> = parts[1].splitn(2, ':').collect();
        if search_parts.len() != 2 {
            return None;
        }

        let search_in = parse_ref_field(search_parts[0].chars().next()?)?;

        Some(FieldReference {
            target_field: target,
            search_in,
            search_value: search_parts[1].to_string(),
        })
    }

    /// Check if a string contains field references.
    pub fn contains_references(text: &str) -> bool {
        text.contains("{REF:")
    }
}

fn parse_ref_field(c: char) -> Option<RefTarget> {
    match c {
        'T' | 't' => Some(RefTarget::Title),
        'U' | 'u' => Some(RefTarget::UserName),
        'P' | 'p' => Some(RefTarget::Password),
        'A' | 'a' => Some(RefTarget::Url),
        'N' | 'n' => Some(RefTarget::Notes),
        // I = search by ID/UUID, maps to Title as a default search target
        'I' | 'i' => Some(RefTarget::Title),
        _ => None,
    }
}

/// Resolve all field references in a text string.
pub fn resolve_references(text: &str, current_entry: &Entry, all_entries: &[&Entry]) -> String {
    let mut result = text.to_string();

    // Find and replace all {REF:...} patterns
    while let Some(start) = result.find("{REF:") {
        let end = result[start..].find('}').map(|i| start + i + 1);
        if let Some(end) = end {
            let ref_str = &result[start..end];
            if let Some(field_ref) = FieldReference::parse(ref_str) {
                let resolved = resolve_single_reference(&field_ref, current_entry, all_entries);
                result.replace_range(start..end, &resolved);
            } else {
                break; // Invalid reference, stop
            }
        } else {
            break;
        }
    }

    result
}

/// Resolve references in a loaded database using a scoped credential unlock.
pub fn resolve_database_references(
    text: &str,
    database: &Database,
    composite_key: &CompositeKey,
) -> DatabaseResult<Zeroizing<String>> {
    let mut result = Zeroizing::new(text.to_string());
    let mut unlock = database.memory_unlock(composite_key);
    while let Some(start) = result.find("{REF:") {
        let Some(end) = result[start..].find('}').map(|index| start + index + 1) else {
            break;
        };
        let Some(reference) = FieldReference::parse(&result[start..end]) else {
            break;
        };
        let mut resolved = Zeroizing::new(String::new());
        for entry in database.entries.values() {
            let matches = match reference.search_in {
                RefTarget::Title => {
                    entry.with_memory_field(&mut unlock, &MemoryField::Title, |value| {
                        Ok(value == reference.search_value)
                    })?
                }
                _ => false,
            };
            if !matches {
                continue;
            }
            let field = match reference.target_field {
                RefTarget::Title => MemoryField::Title,
                RefTarget::UserName => MemoryField::UserName,
                RefTarget::Password => MemoryField::Password,
                RefTarget::Url => MemoryField::Url,
                RefTarget::Notes => MemoryField::Notes,
                RefTarget::CustomField => break,
            };
            resolved = Zeroizing::new(
                entry.with_memory_field(&mut unlock, &field, |value| Ok(value.to_string()))?,
            );
            break;
        }
        result.replace_range(start..end, &resolved);
    }
    Ok(result)
}

fn resolve_single_reference(
    field_ref: &FieldReference,
    _current_entry: &Entry,
    all_entries: &[&Entry],
) -> String {
    for entry in all_entries {
        // Search by ID or title depending on the search field
        let matches = match field_ref.search_in {
            RefTarget::Title => entry.title().as_str() == field_ref.search_value,
            _ => false, // Simplified for now
        };

        if matches {
            return match field_ref.target_field {
                RefTarget::Title => entry.title().as_str().to_string(),
                RefTarget::UserName => entry.username().as_str().to_string(),
                RefTarget::Password => entry.password().as_str().to_string(),
                RefTarget::Url => entry.url().as_str().to_string(),
                RefTarget::Notes => entry.notes().as_str().to_string(),
                RefTarget::CustomField => String::new(),
            };
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_field_reference() {
        let r = FieldReference::parse("{REF:P@I:1234}").unwrap();
        assert_eq!(r.target_field, RefTarget::Password);
    }

    #[test]
    fn test_invalid_reference() {
        assert!(FieldReference::parse("not a ref").is_none());
        assert!(FieldReference::parse("{INVALID}").is_none());
    }
}
