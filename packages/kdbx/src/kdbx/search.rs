//! Search engine
//!

use regex::Regex;

use crate::model::entry::Entry;
use crate::model::core::node::NodeId;

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
    /// Search entries matching the given parameters.
    pub fn search_entries(
        entries: &[&Entry],
        params: &SearchParameters,
    ) -> Vec<SearchResult> {
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
                Self::score_entry_regex(entry, regex.as_ref().expect("checked is_some above"), params)
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
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    /// Quick check if an entry matches a search string.
    pub fn entry_matches(entry: &Entry, query: &str, case_sensitive: bool) -> bool {
        let q = if case_sensitive { query.to_string() } else { query.to_lowercase() };

        let check = |s: &str| -> bool {
            if case_sensitive { s.contains(&q) } else { s.to_lowercase().contains(&q) }
        };

        check(&entry.title)
            || check(entry.username.as_str())
            || check(&entry.url)
            || check(entry.notes.as_str())
    }

    /// Check if an entry matches a regex pattern.
    pub fn entry_matches_regex(entry: &Entry, pattern: &str) -> bool {
        let re = match Regex::new(pattern) {
            Ok(re) => re,
            Err(_) => return false,
        };

        re.is_match(&entry.title)
            || re.is_match(entry.username.as_str())
            || re.is_match(&entry.url)
            || re.is_match(entry.notes.as_str())
    }

    fn score_entry_plain(entry: &Entry, query: &str, params: &SearchParameters) -> f64 {
        let mut score = 0.0f64;

        if params.search_in_title && field_matches(&entry.title, query, params.case_sensitive) {
            score += 2.0;
        }

        if params.search_in_username && field_matches(entry.username.as_str(), query, params.case_sensitive) {
            score += 1.5;
        }

        if params.search_in_url && field_matches(&entry.url, query, params.case_sensitive) {
            score += 1.0;
        }

        if params.search_in_notes && field_matches(entry.notes.as_str(), query, params.case_sensitive) {
            score += 0.5;
        }

        if params.search_in_password && field_matches(entry.password.as_str(), query, params.case_sensitive) {
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
            for field in &entry.custom_fields {
                if field_matches(field.value.as_str(), query, params.case_sensitive) {
                    score += 0.5;
                    break;
                }
            }
        }

        score
    }

    fn score_entry_regex(entry: &Entry, re: &Regex, params: &SearchParameters) -> f64 {
        let mut score = 0.0f64;

        if params.search_in_title && re.is_match(&entry.title) {
            score += 2.0;
        }
        if params.search_in_username && re.is_match(entry.username.as_str()) {
            score += 1.5;
        }
        if params.search_in_url && re.is_match(&entry.url) {
            score += 1.0;
        }
        if params.search_in_notes && re.is_match(entry.notes.as_str()) {
            score += 0.5;
        }
        if params.search_in_password && re.is_match(entry.password.as_str()) {
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
            for field in &entry.custom_fields {
                if re.is_match(field.value.as_str()) {
                    score += 0.5;
                    break;
                }
            }
        }

        score
    }
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
    use crate::model::core::node::NodeId;
    use crate::model::core::security::ProtectedString;

    fn make_entry(id: u8, title: &str, username: &str) -> Entry {
        let mut e = Entry::new(NodeId::from_int(id as i32));
        e.title = title.to_string();
        e.username = ProtectedString::new_plain(username);
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
        e.title = "Test".to_string();
        e.password = ProtectedString::new_protected("super_secret_password");

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
}
