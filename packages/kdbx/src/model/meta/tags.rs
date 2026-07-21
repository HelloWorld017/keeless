//! Tag handling
//!

use serde::{Deserialize, Serialize};

/// A tag with name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
}

impl Tag {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl std::fmt::Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// Parse tags from a semicolon- or comma-separated string.
/// KeePass convention uses ";"; commas are accepted for compatibility.
pub fn parse_tags(tag_string: &str) -> Vec<Tag> {
    tag_string
        .split([';', ','])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(Tag::new)
        .collect()
}

/// Serialize tags to a semicolon-separated string.
pub fn serialize_tags(tags: &[Tag]) -> String {
    tags.iter()
        .map(|t| t.name.as_str())
        .collect::<Vec<_>>()
        .join(";")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_semicolon_and_comma_separated_tags() {
        assert_eq!(
            parse_tags("work, personal; shared, ,archive"),
            [
                Tag::new("work"),
                Tag::new("personal"),
                Tag::new("shared"),
                Tag::new("archive"),
            ]
        );
    }
}
