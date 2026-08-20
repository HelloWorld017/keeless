//! Search engine.
//!

use regex::Regex;

use crate::crypto::memory_protection::{MemoryField, MemoryUnlockSession};
use crate::model::core::node::NodeId;
use crate::model::db::{CompositeKey, Database, EntryFieldSelector};
use crate::model::entry::{memory_field, Entry};
use crate::model::exception::DatabaseResult;

/// Search parameters.
#[derive(Debug, Clone)]
pub struct SearchParameters {
    pub search_string: String,
    pub search_in_title: bool,
    pub search_in_username: bool,
    pub search_in_password: bool,
    pub search_in_url: bool,
    pub search_in_notes: bool,
    pub search_in_other_fields: bool,
    pub search_in_tags: bool,
    pub search_in_group_names: bool,
    pub regex_mode: bool,
    pub exclude_expired: bool,
    pub case_sensitive: bool,
}

impl SearchParameters {
    pub fn new(search_string: &str) -> Self {
        Self {
            search_string: search_string.to_string(),
            search_in_title: true,
            search_in_username: true,
            search_in_password: false,
            search_in_url: true,
            search_in_notes: true,
            search_in_other_fields: true,
            search_in_tags: true,
            search_in_group_names: false,
            regex_mode: false,
            exclude_expired: false,
            case_sensitive: false,
        }
    }

    /// Create parameters for regex search.
    pub fn regex(search_string: &str) -> Self {
        let mut params = Self::new(search_string);
        params.regex_mode = true;
        params
    }
}

/// Search result entry.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub entry_id: NodeId,
    pub score: f64,
}

/// Search helper.
pub struct SearchHelper;

impl SearchHelper {
    /// Search a loaded database, skipping sealed fields when credentials are omitted.
    pub fn search_database(
        database: &Database,
        composite_key: Option<&CompositeKey>,
        params: &SearchParameters,
    ) -> DatabaseResult<Vec<SearchResult>> {
        let Some(composite_key) = composite_key else {
            let entries = database.entries.values().collect::<Vec<_>>();
            return Ok(Self::search_entries(&entries, params));
        };
        let query = if params.case_sensitive {
            params.search_string.clone()
        } else {
            params.search_string.to_lowercase()
        };
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let regex = if params.regex_mode {
            match Regex::new(&params.search_string) {
                Ok(regex) => Some(regex),
                Err(_) => return Ok(Vec::new()),
            }
        } else {
            None
        };
        let mut unlock = database.memory_unlock(composite_key);
        let mut results = Vec::new();
        for entry in database.entries.values() {
            let score = if let Some(regex) = &regex {
                score_memory_entry_regex(entry, &mut unlock, regex, params)?
            } else {
                score_memory_entry_plain(entry, &mut unlock, &query, params)?
            };
            if score > 0.0 {
                results.push(SearchResult {
                    entry_id: entry.id,
                    score,
                });
            }
        }
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(results)
    }

    /// Search entries matching the given parameters.
    pub fn search_entries(entries: &[&Entry], params: &SearchParameters) -> Vec<SearchResult> {
        let query = if params.case_sensitive {
            params.search_string.clone()
        } else {
            params.search_string.to_lowercase()
        };

        if query.is_empty() {
            return Vec::new();
        }

        // Compile regex if in regex mode
        let regex = if params.regex_mode {
            match Regex::new(&params.search_string) {
                Ok(re) => Some(re),
                Err(_) => return Vec::new(), // Invalid regex → no results
            }
        } else {
            None
        };

        let mut results = Vec::new();

        for entry in entries {
            let score = if params.regex_mode {
                Self::score_entry_regex(
                    entry,
                    regex.as_ref().expect("checked is_some above"),
                    params,
                )
            } else {
                Self::score_entry_plain(entry, &query, params)
            };

            if score > 0.0 {
                results.push(SearchResult {
                    entry_id: entry.id,
                    score,
                });
            }
        }

        // Sort by score descending
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    /// Quick check if an entry matches a search string.
    pub fn entry_matches(entry: &Entry, query: &str, case_sensitive: bool) -> bool {
        let q = if case_sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };

        let check = |s: &str| -> bool {
            if case_sensitive {
                s.contains(&q)
            } else {
                s.to_lowercase().contains(&q)
            }
        };

        entry
            .with_unsealed_field(&EntryFieldSelector::Title, check)
            .unwrap_or(false)
            || entry
                .with_unsealed_field(&EntryFieldSelector::UserName, check)
                .unwrap_or(false)
            || entry
                .with_unsealed_field(&EntryFieldSelector::Url, check)
                .unwrap_or(false)
            || entry
                .with_unsealed_field(&EntryFieldSelector::Notes, check)
                .unwrap_or(false)
    }

    /// Check if an entry matches a regex pattern.
    pub fn entry_matches_regex(entry: &Entry, pattern: &str) -> bool {
        let re = match Regex::new(pattern) {
            Ok(re) => re,
            Err(_) => return false,
        };

        entry
            .with_unsealed_field(&EntryFieldSelector::Title, |value| re.is_match(value))
            .unwrap_or(false)
            || entry
                .with_unsealed_field(&EntryFieldSelector::UserName, |value| re.is_match(value))
                .unwrap_or(false)
            || entry
                .with_unsealed_field(&EntryFieldSelector::Url, |value| re.is_match(value))
                .unwrap_or(false)
            || entry
                .with_unsealed_field(&EntryFieldSelector::Notes, |value| re.is_match(value))
                .unwrap_or(false)
    }

    fn score_entry_plain(entry: &Entry, query: &str, params: &SearchParameters) -> f64 {
        let mut score = 0.0f64;

        if params.search_in_title
            && unsealed_field_matches(
                entry,
                &EntryFieldSelector::Title,
                query,
                params.case_sensitive,
            )
        {
            score += 2.0;
        }

        if params.search_in_username
            && unsealed_field_matches(
                entry,
                &EntryFieldSelector::UserName,
                query,
                params.case_sensitive,
            )
        {
            score += 1.5;
        }

        if params.search_in_url
            && unsealed_field_matches(
                entry,
                &EntryFieldSelector::Url,
                query,
                params.case_sensitive,
            )
        {
            score += 1.0;
        }

        if params.search_in_notes
            && unsealed_field_matches(
                entry,
                &EntryFieldSelector::Notes,
                query,
                params.case_sensitive,
            )
        {
            score += 0.5;
        }

        if params.search_in_password
            && unsealed_field_matches(
                entry,
                &EntryFieldSelector::Password,
                query,
                params.case_sensitive,
            )
        {
            score += 1.0;
        }

        if params.search_in_tags {
            for tag in &entry.tags {
                if field_matches(tag, query, params.case_sensitive) {
                    score += 1.0;
                    break;
                }
            }
        }

        if params.search_in_other_fields {
            for (_, field) in entry.custom_fields() {
                if field
                    .value
                    .as_unsealed_str()
                    .is_some_and(|value| field_matches(value, query, params.case_sensitive))
                {
                    score += 0.5;
                    break;
                }
            }
        }

        score
    }

    fn score_entry_regex(entry: &Entry, re: &Regex, params: &SearchParameters) -> f64 {
        let mut score = 0.0f64;

        if params.search_in_title
            && unsealed_field_matches_regex(entry, &EntryFieldSelector::Title, re)
        {
            score += 2.0;
        }
        if params.search_in_username
            && unsealed_field_matches_regex(entry, &EntryFieldSelector::UserName, re)
        {
            score += 1.5;
        }
        if params.search_in_url && unsealed_field_matches_regex(entry, &EntryFieldSelector::Url, re)
        {
            score += 1.0;
        }
        if params.search_in_notes
            && unsealed_field_matches_regex(entry, &EntryFieldSelector::Notes, re)
        {
            score += 0.5;
        }
        if params.search_in_password
            && unsealed_field_matches_regex(entry, &EntryFieldSelector::Password, re)
        {
            score += 1.0;
        }
        if params.search_in_tags {
            for tag in &entry.tags {
                if re.is_match(tag) {
                    score += 1.0;
                    break;
                }
            }
        }
        if params.search_in_other_fields {
            for (_, field) in entry.custom_fields() {
                if field
                    .value
                    .as_unsealed_str()
                    .is_some_and(|value| re.is_match(value))
                {
                    score += 0.5;
                    break;
                }
            }
        }

        score
    }
}

fn unsealed_field_matches(
    entry: &Entry,
    selector: &EntryFieldSelector,
    query: &str,
    case_sensitive: bool,
) -> bool {
    entry
        .with_unsealed_field(selector, |value| {
            field_matches(value, query, case_sensitive)
        })
        .unwrap_or(false)
}

fn unsealed_field_matches_regex(
    entry: &Entry,
    selector: &EntryFieldSelector,
    regex: &Regex,
) -> bool {
    entry
        .with_unsealed_field(selector, |value| regex.is_match(value))
        .unwrap_or(false)
}

fn score_memory_entry_plain(
    entry: &Entry,
    unlock: &mut MemoryUnlockSession<'_>,
    query: &str,
    params: &SearchParameters,
) -> DatabaseResult<f64> {
    let mut score = 0.0;
    for (enabled, weight, field) in [
        (params.search_in_title, 2.0, MemoryField::Title),
        (params.search_in_username, 1.5, MemoryField::UserName),
        (params.search_in_url, 1.0, MemoryField::Url),
        (params.search_in_notes, 0.5, MemoryField::Notes),
        (params.search_in_password, 1.0, MemoryField::Password),
    ] {
        if enabled {
            entry.with_memory_field(unlock, &field, |value| {
                if field_matches(value, query, params.case_sensitive) {
                    score += weight;
                }
                Ok(())
            })?;
        }
    }
    if params.search_in_tags
        && entry
            .tags
            .iter()
            .any(|tag| field_matches(tag, query, params.case_sensitive))
    {
        score += 1.0;
    }
    if params.search_in_other_fields {
        for (id, custom) in entry.custom_fields() {
            let field = memory_field(id, &custom.name);
            let matched = custom
                .value
                .with_plaintext(unlock, entry.id, &field, |value| {
                    Ok(field_matches(value, query, params.case_sensitive))
                })?;
            if matched {
                score += 0.5;
                break;
            }
        }
    }
    Ok(score)
}

fn score_memory_entry_regex(
    entry: &Entry,
    unlock: &mut MemoryUnlockSession<'_>,
    regex: &Regex,
    params: &SearchParameters,
) -> DatabaseResult<f64> {
    let mut score = 0.0;
    for (enabled, weight, field) in [
        (params.search_in_title, 2.0, MemoryField::Title),
        (params.search_in_username, 1.5, MemoryField::UserName),
        (params.search_in_url, 1.0, MemoryField::Url),
        (params.search_in_notes, 0.5, MemoryField::Notes),
        (params.search_in_password, 1.0, MemoryField::Password),
    ] {
        if enabled {
            entry.with_memory_field(unlock, &field, |value| {
                if regex.is_match(value) {
                    score += weight;
                }
                Ok(())
            })?;
        }
    }
    if params.search_in_tags && entry.tags.iter().any(|tag| regex.is_match(tag)) {
        score += 1.0;
    }
    if params.search_in_other_fields {
        for (id, custom) in entry.custom_fields() {
            let field = memory_field(id, &custom.name);
            let matched = custom
                .value
                .with_plaintext(unlock, entry.id, &field, |value| Ok(regex.is_match(value)))?;
            if matched {
                score += 0.5;
                break;
            }
        }
    }
    Ok(score)
}

fn field_matches(field: &str, query: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        field.contains(query)
    } else {
        field.to_lowercase().contains(query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kdbx::kdf::aes_kdf::AES_KDF_UUID;
    use crate::kdbx::kdf::KdfParameters;
    use crate::model::core::node::NodeId;
    use crate::model::core::security::ProtectedString;
    use crate::model::db::{CompositeCredentials, DatabaseVersion};

    fn make_entry(id: u8, title: &str, username: &str) -> Entry {
        let mut e = Entry::new(NodeId::from_int(id as i32));
        e.set_title(title);
        e.set_username(ProtectedString::new_plain(username));
        e
    }

    #[test]
    fn test_search_by_title() {
        let e1 = make_entry(1, "Gmail Account", "user@gmail.com");
        let e2 = make_entry(2, "GitHub", "dev@github.com");
        let entries: Vec<&Entry> = vec![&e1, &e2];

        let params = SearchParameters::new("gmail");
        let results = SearchHelper::search_entries(&entries, &params);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_case_insensitive() {
        let e1 = make_entry(1, "MyBank Account", "user");
        let entries: Vec<&Entry> = vec![&e1];

        let params = SearchParameters::new("mybank");
        let results = SearchHelper::search_entries(&entries, &params);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_entry_matches() {
        let e = make_entry(1, "Test Entry", "user");
        assert!(SearchHelper::entry_matches(&e, "test", false));
        assert!(!SearchHelper::entry_matches(&e, "nonexistent", false));
    }

    #[test]
    fn test_regex_search() {
        let e1 = make_entry(1, "Gmail Account", "user@gmail.com");
        let e2 = make_entry(2, "GitHub", "dev@github.com");
        let e3 = make_entry(3, "AWS Console", "admin@aws.amazon.com");
        let entries: Vec<&Entry> = vec![&e1, &e2, &e3];

        // Match all entries with email-like usernames
        let params = SearchParameters::regex(r"@gmail\.com$");
        let results = SearchHelper::search_entries(&entries, &params);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry_id, NodeId::from_int(1));
    }

    #[test]
    fn test_regex_search_or_pattern() {
        let e1 = make_entry(1, "Gmail", "a@b.com");
        let e2 = make_entry(2, "GitHub", "c@d.com");
        let e3 = make_entry(3, "AWS", "e@f.com");
        let entries: Vec<&Entry> = vec![&e1, &e2, &e3];

        let params = SearchParameters::regex("Gmail|AWS");
        let results = SearchHelper::search_entries(&entries, &params);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_regex_search_invalid_pattern() {
        let e1 = make_entry(1, "Test", "user");
        let entries: Vec<&Entry> = vec![&e1];

        let params = SearchParameters::regex("[invalid");
        let results = SearchHelper::search_entries(&entries, &params);
        assert!(results.is_empty()); // Invalid regex → no results
    }

    #[test]
    fn test_entry_matches_regex() {
        let e = make_entry(1, "MyBank_2024", "user");
        assert!(SearchHelper::entry_matches_regex(&e, r"\d{4}"));
        assert!(SearchHelper::entry_matches_regex(&e, "Bank"));
        assert!(!SearchHelper::entry_matches_regex(&e, r"^GitHub"));
    }

    #[test]
    fn test_search_password_field() {
        let mut e = Entry::new(NodeId::from_int(1));
        e.set_title("Test");
        e.set_password(ProtectedString::new_protected("super_secret_password"));

        let entries: Vec<&Entry> = vec![&e];

        // Password not searched by default
        let params = SearchParameters::new("secret");
        let results = SearchHelper::search_entries(&entries, &params);
        assert_eq!(results.len(), 0); // Password field not included

        // Search in password
        let mut params = SearchParameters::new("secret");
        params.search_in_password = true;
        params.search_in_title = false;
        params.search_in_username = false;
        params.search_in_url = false;
        params.search_in_notes = false;
        let results = SearchHelper::search_entries(&entries, &params);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_database_unlocks_protected_values_for_the_operation() {
        let mut database = Database::new(DatabaseVersion::KDBX4);
        let mut parameters = KdfParameters::new(AES_KDF_UUID);
        parameters.set_byte_array("S", &[0x22; 32]);
        parameters.set_uint64("R", 1);
        database.kdf_parameters = Some(parameters);
        let entry_id = NodeId::new_uuid();
        let mut entry = Entry::new(entry_id);
        entry.set_password(ProtectedString::new_protected("needle-secret"));
        database.entries.insert(entry_id, entry);
        let key = CompositeCredentials::new()
            .with_password(b"search password")
            .unwrap()
            .derive_key(database.kdf_parameters.as_ref().unwrap())
            .unwrap();
        database.protect_entry_strings(&key).unwrap();

        let mut params = SearchParameters::new("needle");
        params.search_in_title = false;
        params.search_in_username = false;
        params.search_in_password = true;
        params.search_in_url = false;
        params.search_in_notes = false;
        params.search_in_other_fields = false;
        let results = SearchHelper::search_database(&database, Some(&key), &params).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry_id, entry_id);

        let wrong = CompositeCredentials::new()
            .with_password(b"wrong")
            .unwrap()
            .derive_key(database.kdf_parameters.as_ref().unwrap())
            .unwrap();
        assert!(SearchHelper::search_database(&database, Some(&wrong), &params).is_err());
    }

    #[test]
    fn searches_unsealed_fields_without_credentials() {
        let mut database = Database::new(DatabaseVersion::KDBX4);
        let mut parameters = KdfParameters::new(AES_KDF_UUID);
        parameters.set_byte_array("S", &[0x33; 32]);
        parameters.set_uint64("R", 1);
        database.kdf_parameters = Some(parameters);

        let sealed_id = NodeId::new_uuid();
        let mut sealed = Entry::new(sealed_id);
        sealed.set_title(ProtectedString::new_protected("needle title"));
        sealed.set_username(ProtectedString::new_protected("needle username"));
        sealed.set_password(ProtectedString::new_protected("needle password"));
        sealed.set_url(ProtectedString::new_protected("https://needle.example"));
        sealed.set_notes(ProtectedString::new_protected("needle notes"));
        sealed.add_custom_field("Secret", ProtectedString::new_protected("needle custom"));
        database.entries.insert(sealed_id, sealed);

        let key = CompositeCredentials::new()
            .with_password(b"search password")
            .unwrap()
            .derive_key(database.kdf_parameters.as_ref().unwrap())
            .unwrap();
        database.protect_entry_strings(&key).unwrap();

        let unsealed_id = NodeId::new_uuid();
        let mut unsealed = Entry::new(unsealed_id);
        unsealed.set_title(ProtectedString::new_protected("needle title"));
        unsealed.set_username(ProtectedString::new_protected("needle username"));
        unsealed.set_password(ProtectedString::new_protected("needle password"));
        unsealed.set_url(ProtectedString::new_protected("https://needle.example"));
        unsealed.set_notes(ProtectedString::new_protected("needle notes"));
        unsealed.add_custom_field("Secret", ProtectedString::new_protected("needle custom"));
        database.entries.insert(unsealed_id, unsealed);

        let mut params = SearchParameters::new("needle");
        params.search_in_password = true;
        let results = SearchHelper::search_database(&database, None, &params).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry_id, unsealed_id);

        let results = SearchHelper::search_database(&database, Some(&key), &params).unwrap();
        assert_eq!(results.len(), 2);

        let entries = database.entries.values().collect::<Vec<_>>();
        let results = SearchHelper::search_entries(&entries, &params);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry_id, unsealed_id);

        let regex = SearchParameters::regex("needle");
        let results = SearchHelper::search_database(&database, None, &regex).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry_id, unsealed_id);

        let sealed = &database.entries[&sealed_id];
        let unsealed = &database.entries[&unsealed_id];
        assert_eq!(
            sealed.with_unsealed_field(&EntryFieldSelector::Title, str::to_string),
            None
        );
        assert_eq!(
            unsealed.with_unsealed_field(&EntryFieldSelector::Title, str::to_string),
            Some("needle title".into())
        );
        assert!(!SearchHelper::entry_matches(sealed, "needle", false));
        assert!(SearchHelper::entry_matches(unsealed, "needle", false));
        assert!(!SearchHelper::entry_matches_regex(sealed, "needle"));
        assert!(SearchHelper::entry_matches_regex(unsealed, "needle"));
    }
}
