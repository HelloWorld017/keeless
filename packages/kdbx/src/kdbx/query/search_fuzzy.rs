//! Fuzzy search and query parsing shared by all hosts.

use std::collections::HashSet;

use icu_normalizer::DecomposingNormalizer;
use nucleo_matcher::{
    pattern::{CaseMatching, Normalization, Pattern},
    Config, Matcher, Utf32Str,
};

use crate::crypto::memory_protection::{MemoryField, MemoryUnlockSession};
use crate::model::core::node::NodeId;
use crate::model::db::{CompositeKey, Database, EntryFieldSelector};
use crate::model::entry::Entry;
use crate::model::exception::DatabaseResult;

/// A recognized search filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchFilter {
    In(String),
    Tag(String),
}

/// Parsed user search input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub filters: Vec<SearchFilter>,
    pub free_text: String,
    /// Original recognized filter tokens, used to complete filters without reparsing in the UI.
    pub filter_tokens: Vec<String>,
}

impl SearchQuery {
    pub fn parse(input: &str) -> Self {
        let Some(tokens) = tokenize(input) else {
            return Self {
                filters: Vec::new(),
                free_text: input.to_string(),
                filter_tokens: Vec::new(),
            };
        };
        let mut filters = Vec::new();
        let mut free_text = Vec::new();
        let mut filter_tokens = Vec::new();

        for token in tokens {
            let filter = token
                .value
                .strip_prefix("in:")
                .and_then(|value| (!value.is_empty()).then(|| SearchFilter::In(value.to_string())))
                .or_else(|| {
                    token.value.strip_prefix("tag:").and_then(|value| {
                        (!value.is_empty()).then(|| SearchFilter::Tag(value.to_string()))
                    })
                });
            if let Some(filter) = filter {
                filters.push(filter);
                filter_tokens.push(token.raw);
            } else {
                free_text.push(token.value);
            }
        }

        Self {
            filters,
            free_text: free_text.join(" "),
            filter_tokens,
        }
    }
}

struct Token {
    raw: String,
    value: String,
}

fn tokenize(input: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut raw = String::new();
    let mut value = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for character in input.chars() {
        if escaped {
            raw.push(character);
            value.push(character);
            escaped = false;
            continue;
        }
        if quoted && character == '\\' {
            raw.push(character);
            escaped = true;
            continue;
        }
        if character == '"' {
            raw.push(character);
            quoted = !quoted;
            continue;
        }
        if !quoted && character.is_whitespace() {
            if !raw.is_empty() {
                tokens.push(Token {
                    raw: std::mem::take(&mut raw),
                    value: std::mem::take(&mut value),
                });
            }
            continue;
        }
        raw.push(character);
        value.push(character);
    }
    if quoted || escaped {
        return None;
    }
    if !raw.is_empty() {
        tokens.push(Token { raw, value });
    }
    Some(tokens)
}

/// An entry match ordered by relevance and original candidate order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzySearchResult {
    pub entry_id: NodeId,
    pub score: u32,
    pub field_priority: u8,
    pub index: usize,
}

/// Reusable fuzzy matcher for a single request.
pub struct FuzzySearchHelper {
    pattern: Option<Pattern>,
    normalizer: DecomposingNormalizer,
    matcher: Matcher,
    utf32_buffer: Vec<char>,
}

impl FuzzySearchHelper {
    pub fn new(query: &SearchQuery) -> Self {
        let normalizer = DecomposingNormalizer::new_nfkd();
        let normalized = normalizer.normalize(&query.free_text);
        Self {
            pattern: (!normalized.is_empty())
                .then(|| Pattern::parse(&normalized, CaseMatching::Ignore, Normalization::Never)),
            normalizer,
            matcher: Matcher::new(Config::DEFAULT),
            utf32_buffer: Vec::new(),
        }
    }

    /// Scores a single string using the request's matcher scratch space.
    pub fn score(&mut self, value: &str) -> Option<u32> {
        let Some(pattern) = self.pattern.as_ref() else {
            return Some(0);
        };
        let normalized = self.normalizer.normalize(value);
        pattern.score(
            Utf32Str::new(&normalized, &mut self.utf32_buffer),
            &mut self.matcher,
        )
    }

    pub fn has_free_text(&self) -> bool {
        self.pattern.is_some()
    }

    /// Searches only the allowed entry fields after applying exact query filters.
    pub fn search_entries(
        &mut self,
        database: &Database,
        entries: &[&Entry],
        composite_key: Option<&CompositeKey>,
        query: &SearchQuery,
    ) -> DatabaseResult<Vec<FuzzySearchResult>> {
        let group_filters = query
            .filters
            .iter()
            .filter_map(|filter| match filter {
                SearchFilter::In(name) => Some(name),
                SearchFilter::Tag(_) => None,
            })
            .map(|name| {
                database
                    .groups
                    .values()
                    .filter(|group| group.title == *name)
                    .flat_map(|group| group.child_entry_ids.iter().copied())
                    .collect::<HashSet<_>>()
            })
            .collect::<Vec<_>>();
        if group_filters.iter().any(HashSet::is_empty) {
            return Ok(Vec::new());
        }
        let tag_filters = query
            .filters
            .iter()
            .filter_map(|filter| match filter {
                SearchFilter::Tag(name) => Some(name.as_str()),
                SearchFilter::In(_) => None,
            })
            .collect::<Vec<_>>();

        let mut unlock = composite_key.map(|key| database.memory_unlock(key));
        let mut results = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            if !group_filters.iter().all(|ids| ids.contains(&entry.id))
                || !tag_filters
                    .iter()
                    .all(|filter| entry.tags.iter().any(|tag| tag.trim() == *filter))
            {
                continue;
            }
            let Some((score, field_priority)) = self.entry_score(entry, unlock.as_mut())? else {
                continue;
            };
            results.push(FuzzySearchResult {
                entry_id: entry.id,
                score,
                field_priority,
                index,
            });
        }
        results.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| right.field_priority.cmp(&left.field_priority))
                .then_with(|| left.index.cmp(&right.index))
        });
        Ok(results)
    }

    fn entry_score(
        &mut self,
        entry: &Entry,
        unlock: Option<&mut MemoryUnlockSession<'_>>,
    ) -> DatabaseResult<Option<(u32, u8)>> {
        if !self.has_free_text() {
            return Ok(Some((0, 0)));
        }
        let mut best = None;
        let mut check = |value: &str, priority| {
            if let Some(score) = self.score(value) {
                if best.is_none_or(|(best_score, best_priority)| {
                    score > best_score || (score == best_score && priority > best_priority)
                }) {
                    best = Some((score, priority));
                }
            }
        };

        if let Some(unlock) = unlock {
            entry.with_memory_field(unlock, &MemoryField::Title, |value| {
                check(value, 3);
                Ok(())
            })?;
            entry.with_memory_field(unlock, &MemoryField::UserName, |value| {
                check(value, 2);
                Ok(())
            })?;
            entry.with_memory_field(unlock, &MemoryField::Url, |value| {
                check(value, 1);
                Ok(())
            })?;
        } else {
            for (selector, priority) in [
                (EntryFieldSelector::Title, 3),
                (EntryFieldSelector::UserName, 2),
                (EntryFieldSelector::Url, 1),
            ] {
                entry.with_unsealed_field(&selector, |value| check(value, priority));
            }
        }
        for tag in &entry.tags {
            check(tag, 1);
        }
        Ok(best)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_filters_and_preserves_their_original_tokens() {
        assert_eq!(
            SearchQuery::parse(r#"tag:passkey in:"Social Media" insta gram"#),
            SearchQuery {
                filters: vec![
                    SearchFilter::Tag("passkey".into()),
                    SearchFilter::In("Social Media".into()),
                ],
                free_text: "insta gram".into(),
                filter_tokens: vec!["tag:passkey".into(), r#"in:"Social Media""#.into()],
            }
        );
    }

    #[test]
    fn treats_incomplete_and_empty_filters_as_free_text() {
        assert_eq!(SearchQuery::parse("tag:").free_text, "tag:");
        assert_eq!(SearchQuery::parse("in:\"Social").free_text, "in:\"Social");
        assert_eq!(SearchQuery::parse("type:login").free_text, "type:login");
    }

    #[test]
    fn nfkd_matches_diacritics_and_hangul_jamo() {
        let query = SearchQuery::parse("cafe com");
        let mut helper = FuzzySearchHelper::new(&query);
        assert!(helper.score("caf\u{00e9}.com").is_some());
        assert!(helper.score("caf\u{0065}\u{0301}.com").is_some());
        assert!(helper.score("cafe.example").is_none());

        let query = SearchQuery::parse("\u{3137}\u{3139}\u{3148}");
        let mut helper = FuzzySearchHelper::new(&query);
        assert!(helper.score("\u{B2E4}\u{B78C}\u{C950}").is_some());
    }
}
