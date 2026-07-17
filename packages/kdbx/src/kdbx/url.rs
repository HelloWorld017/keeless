//! URL matching for entry URLs and KeePass2Android additional URL fields.

use regex::{Regex, RegexBuilder};
use url::{Host, Url};

use crate::crypto::memory_protection::MemoryField;
use crate::model::core::node::NodeId;
use crate::model::db::{CompositeKey, Database};
use crate::model::entry::Entry;
use crate::model::exception::DatabaseResult;

const ADDITIONAL_URL_PREFIX: &str = "KP2A_URL_";
const WILDCARD_TOKEN: &str = "keelesswildcardtoken";

const SCORE_EXACT: f64 = 100.0;
const SCORE_WITHOUT_QUERY: f64 = 90.0;
const SCORE_PARENT_PATH: f64 = 85.0;
const SCORE_EXACT_HOST: f64 = 80.0;
const SCORE_PARENT_HOST: f64 = 60.0;

/// Parameters for matching entries against a URL.
#[derive(Debug, Clone)]
pub struct UrlMatchParameters {
    pub url: String,
    /// Require stored URLs with a scheme to use the same scheme as the target URL.
    pub match_scheme: bool,
}

impl UrlMatchParameters {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            match_scheme: false,
        }
    }
}

/// An entry matched against a URL.
#[derive(Debug, Clone, PartialEq)]
pub struct UrlMatchResult {
    pub entry_id: NodeId,
    pub score: f64,
}

/// Matches entry URLs and KeePass2Android additional URLs.
pub struct UrlMatcher;

impl UrlMatcher {
    /// Match URLs in a loaded database using credential-scoped field access.
    pub fn match_database(
        database: &Database,
        composite_key: &CompositeKey,
        params: &UrlMatchParameters,
    ) -> DatabaseResult<Vec<UrlMatchResult>> {
        let Some(target) = ParsedUrl::parse(&params.url) else {
            return Ok(Vec::new());
        };
        let mut unlock = database.memory_unlock(composite_key);
        let mut results = Vec::new();
        for entry in database.entries.values() {
            let mut best_score =
                entry.with_memory_field(&mut unlock, &MemoryField::Url, |value| {
                    Ok(score_regular_url(value, &target, params.match_scheme))
                })?;
            for field in &entry.custom_fields {
                if !field.name.starts_with(ADDITIONAL_URL_PREFIX) {
                    continue;
                }
                let memory_field = MemoryField::Custom(field.name.clone());
                let score = entry.with_memory_field(&mut unlock, &memory_field, |value| {
                    Ok(score_additional_url(
                        value,
                        &params.url,
                        &target,
                        params.match_scheme,
                    ))
                })?;
                best_score = max_score(best_score, score);
            }
            if let Some(score) = best_score {
                results.push(UrlMatchResult {
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

    /// Match entries against the target URL and sort them by descending score.
    pub fn match_entries(entries: &[&Entry], params: &UrlMatchParameters) -> Vec<UrlMatchResult> {
        let Some(target) = ParsedUrl::parse(&params.url) else {
            return Vec::new();
        };

        let mut results = Vec::new();
        for entry in entries {
            let mut best_score = score_regular_url(&entry.url, &target, params.match_scheme);

            for field in &entry.custom_fields {
                if !field.name.starts_with(ADDITIONAL_URL_PREFIX) {
                    continue;
                }

                let score = score_additional_url(
                    field.value.as_str(),
                    &params.url,
                    &target,
                    params.match_scheme,
                );
                best_score = max_score(best_score, score);
            }

            if let Some(score) = best_score {
                results.push(UrlMatchResult {
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
        results
    }
}

#[derive(Debug)]
struct ParsedUrl {
    url: Url,
    explicit_port: Option<u16>,
}

impl ParsedUrl {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }

        let normalized = if value.contains("://") {
            value.to_string()
        } else {
            format!("https://{value}")
        };
        let url = Url::parse(&normalized).ok()?;
        url.host()?;

        Some(Self {
            url,
            explicit_port: explicit_port(&normalized),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostMatch {
    Exact,
    Parent,
}

fn score_regular_url(stored_value: &str, target: &ParsedUrl, match_scheme: bool) -> Option<f64> {
    let stored = ParsedUrl::parse(stored_value)?;
    let host_match = regular_url_matches(&stored, target, match_scheme)?;

    if stored.url == target.url {
        return Some(SCORE_EXACT);
    }

    if urls_equal_without_query_and_fragment(&stored.url, &target.url) {
        return Some(SCORE_WITHOUT_QUERY);
    }

    if host_match == HostMatch::Exact
        && stored.url.scheme() == target.url.scheme()
        && is_parent_path(stored.url.path(), target.url.path())
    {
        return Some(SCORE_PARENT_PATH);
    }

    Some(match host_match {
        HostMatch::Exact => SCORE_EXACT_HOST,
        HostMatch::Parent => SCORE_PARENT_HOST,
    })
}

fn score_additional_url(
    stored_value: &str,
    target_value: &str,
    target: &ParsedUrl,
    match_scheme: bool,
) -> Option<f64> {
    if let Some(exact) = quoted_value(stored_value) {
        return (exact == target_value).then_some(SCORE_EXACT);
    }

    if stored_value.contains('*') {
        if let Some(pattern) = WildcardUrl::parse(stored_value) {
            return pattern.score(target, match_scheme);
        }
    }

    score_regular_url(stored_value, target, match_scheme)
}

fn regular_url_matches(
    stored: &ParsedUrl,
    target: &ParsedUrl,
    match_scheme: bool,
) -> Option<HostMatch> {
    if match_scheme && stored.url.scheme() != target.url.scheme() {
        return None;
    }

    if !port_matches(stored.explicit_port, target) {
        return None;
    }

    match (stored.url.host()?, target.url.host()?) {
        (Host::Domain(stored_host), Host::Domain(target_host)) => {
            if stored_host.eq_ignore_ascii_case(target_host) {
                Some(HostMatch::Exact)
            } else if target_host
                .to_ascii_lowercase()
                .ends_with(&format!(".{}", stored_host.to_ascii_lowercase()))
            {
                Some(HostMatch::Parent)
            } else {
                None
            }
        }
        (Host::Ipv4(stored_host), Host::Ipv4(target_host)) if stored_host == target_host => {
            Some(HostMatch::Exact)
        }
        (Host::Ipv6(stored_host), Host::Ipv6(target_host)) if stored_host == target_host => {
            Some(HostMatch::Exact)
        }
        _ => None,
    }
}

fn port_matches(stored_port: Option<u16>, target: &ParsedUrl) -> bool {
    stored_port.is_none()
        || stored_port
            == target
                .explicit_port
                .or_else(|| target.url.port_or_known_default())
}

fn urls_equal_without_query_and_fragment(first: &Url, second: &Url) -> bool {
    let mut first = first.clone();
    first.set_query(None);
    first.set_fragment(None);

    let mut second = second.clone();
    second.set_query(None);
    second.set_fragment(None);
    first == second
}

fn is_parent_path(parent: &str, child: &str) -> bool {
    if parent == child || !child.starts_with(parent) {
        return false;
    }
    parent.ends_with('/') || child.as_bytes().get(parent.len()) == Some(&b'/')
}

fn quoted_value(value: &str) -> Option<&str> {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
}

fn explicit_port(value: &str) -> Option<u16> {
    let authority = value.split_once("://")?.1.split(['/', '?', '#']).next()?;
    let host_and_port = authority.rsplit('@').next()?;

    if let Some(bracket_end) = host_and_port.find(']') {
        return host_and_port
            .get(bracket_end + 1..)?
            .strip_prefix(':')?
            .parse()
            .ok();
    }

    let (host, port) = host_and_port.rsplit_once(':')?;
    (!host.contains(':')).then(|| port.parse().ok()).flatten()
}

fn max_score(first: Option<f64>, second: Option<f64>) -> Option<f64> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first.max(second)),
        (Some(score), None) | (None, Some(score)) => Some(score),
        (None, None) => None,
    }
}

#[derive(Debug)]
struct WildcardUrl {
    parsed: ParsedUrl,
    host_pattern: String,
    path_pattern: String,
    host_has_wildcard: bool,
    path_has_wildcard: bool,
}

impl WildcardUrl {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let scheme = value
            .split_once("://")
            .map(|(scheme, _)| scheme)
            .unwrap_or("https");
        if scheme.contains('*') {
            return None;
        }

        let replaced = value.replace('*', WILDCARD_TOKEN);
        let parsed = ParsedUrl::parse(&replaced)?;
        let host_pattern = parsed.url.host_str()?.replace(WILDCARD_TOKEN, "*");
        let path_pattern = parsed.url.path().replace(WILDCARD_TOKEN, "*");
        let host_has_wildcard = host_pattern.contains('*');
        let path_has_wildcard = path_pattern.contains('*');

        (host_has_wildcard || path_has_wildcard).then_some(Self {
            parsed,
            host_pattern,
            path_pattern,
            host_has_wildcard,
            path_has_wildcard,
        })
    }

    fn score(&self, target: &ParsedUrl, match_scheme: bool) -> Option<f64> {
        if match_scheme && self.parsed.url.scheme() != target.url.scheme() {
            return None;
        }
        if !port_matches(self.parsed.explicit_port, target) {
            return None;
        }

        let host = target.url.host_str()?;
        if !wildcard_matches(&self.host_pattern, host, true)
            || !wildcard_matches(&self.path_pattern, target.url.path(), false)
        {
            return None;
        }

        if self.host_has_wildcard {
            Some(SCORE_PARENT_HOST)
        } else if self.path_has_wildcard {
            Some(SCORE_PARENT_PATH)
        } else {
            Some(SCORE_EXACT_HOST)
        }
    }
}

fn wildcard_matches(pattern: &str, value: &str, host: bool) -> bool {
    let expression = pattern
        .split('*')
        .map(regex::escape)
        .collect::<Vec<_>>()
        .join(".*");
    let expression = format!("^{expression}$");
    build_regex(&expression, host).is_some_and(|regex| regex.is_match(value))
}

fn build_regex(expression: &str, case_insensitive: bool) -> Option<Regex> {
    RegexBuilder::new(expression)
        .case_insensitive(case_insensitive)
        .build()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::core::security::ProtectedString;
    use crate::model::entry::EntryField;

    fn entry(id: i32, url: &str) -> Entry {
        let mut entry = Entry::new(NodeId::from_int(id));
        entry.url = url.to_string();
        entry
    }

    fn add_url(entry: &mut Entry, name: &str, url: &str) {
        entry.custom_fields.push(EntryField {
            name: name.to_string(),
            value: ProtectedString::new_plain(url),
            is_protected: false,
        });
    }

    fn matches(entries: &[&Entry], url: &str) -> Vec<UrlMatchResult> {
        UrlMatcher::match_entries(entries, &UrlMatchParameters::new(url))
    }

    #[test]
    fn matches_exact_hosts_and_subdomains() {
        let parent = entry(1, "https://example.com");
        let exact = entry(2, "https://login.example.com");
        let other_subdomain = entry(3, "https://other.example.com");
        let unrelated_suffix = entry(4, "https://ample.com");

        let results = matches(
            &[&parent, &exact, &other_subdomain, &unrelated_suffix],
            "https://login.example.com/path",
        );

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].entry_id, exact.id);
        assert_eq!(results[0].score, SCORE_PARENT_PATH);
        assert_eq!(results[1].entry_id, parent.id);
        assert_eq!(results[1].score, SCORE_PARENT_HOST);
    }

    #[test]
    fn ranks_url_details() {
        let exact = entry(1, "https://example.com/login?flow=1#step");
        let without_query = entry(2, "https://example.com/login");
        let parent_path = entry(3, "https://example.com/");
        let same_host = entry(4, "https://example.com/other");

        let results = matches(
            &[&same_host, &parent_path, &without_query, &exact],
            "https://example.com/login?flow=1#step",
        );

        assert_eq!(
            results
                .iter()
                .map(|result| result.score)
                .collect::<Vec<_>>(),
            vec![
                SCORE_EXACT,
                SCORE_WITHOUT_QUERY,
                SCORE_PARENT_PATH,
                SCORE_EXACT_HOST,
            ]
        );
    }

    #[test]
    fn optionally_matches_scheme_and_requires_stored_port() {
        let http = entry(1, "http://example.com/login");
        let matching_port = entry(2, "https://example.com:8443/login");
        let wrong_port = entry(3, "https://example.com:9443/login");
        let entries = [&http, &matching_port, &wrong_port];

        let results = matches(&entries, "https://example.com:8443/login");
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|result| result.entry_id == http.id));
        assert!(results
            .iter()
            .any(|result| result.entry_id == matching_port.id));

        let mut params = UrlMatchParameters::new("https://example.com:8443/login");
        params.match_scheme = true;
        let results = UrlMatcher::match_entries(&entries, &params);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry_id, matching_port.id);
    }

    #[test]
    fn supports_additional_exact_and_wildcard_urls() {
        let mut exact = entry(1, "https://unrelated.test");
        add_url(
            &mut exact,
            "KP2A_URL_1",
            "\"https://example.com/login?flow=1\"",
        );

        let mut wildcard = entry(2, "https://unrelated.test");
        add_url(&mut wildcard, "KP2A_URL_2", "https://*.example.com/login/*");

        let results = matches(
            &[&wildcard, &exact],
            "https://accounts.example.com/login/start",
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry_id, wildcard.id);
        assert_eq!(results[0].score, SCORE_PARENT_HOST);

        let results = matches(&[&wildcard, &exact], "https://example.com/login?flow=1");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry_id, exact.id);
        assert_eq!(results[0].score, SCORE_EXACT);
    }

    #[test]
    fn additional_url_match_uses_highest_score() {
        let mut candidate = entry(1, "https://example.com/");
        add_url(
            &mut candidate,
            "KP2A_URL_login",
            "https://example.com/login",
        );
        add_url(&mut candidate, "not_KP2A_URL_1", "https://other.test");

        let results = matches(&[&candidate], "https://example.com/login");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, SCORE_EXACT);
    }

    #[test]
    fn primary_url_does_not_enable_special_syntax() {
        let quoted = entry(1, "\"https://example.com/login\"");
        let wildcard = entry(2, "https://*.example.com/*");

        assert!(matches(&[&quoted, &wildcard], "https://a.example.com/login").is_empty());
    }

    #[test]
    fn rejects_empty_and_invalid_target_urls() {
        let candidate = entry(1, "https://example.com");
        assert!(matches(&[&candidate], "").is_empty());
        assert!(matches(&[&candidate], "://invalid").is_empty());
    }
}
