//! Enhanced Entry Templates (ETM) custom-field metadata.

use std::collections::HashMap;

use uuid::Uuid;

use crate::model::entry::{Entry, StandardField};

pub const ETM_PREFIX: &str = "_etm_";
pub const ETM_TEMPLATE: &str = "_etm_template";
pub const ETM_TEMPLATE_UUID: &str = "_etm_template_uuid";
pub const ETM_TITLE_PREFIX: &str = "_etm_title_";
pub const ETM_TYPE_PREFIX: &str = "_etm_type_";
pub const ETM_POSITION_PREFIX: &str = "_etm_position_";
pub const ETM_OPTIONS_PREFIX: &str = "_etm_options_";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtmFieldType {
    Inline,
    InlineUrl,
    Popout,
    ProtectedInline,
    ProtectedPopout,
    RichTextbox,
    Date,
    Time,
    DateTime,
    Checkbox,
    Listbox,
    Divider,
}

impl EtmFieldType {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "Inline" => Some(Self::Inline),
            "Inline URL" => Some(Self::InlineUrl),
            "Popout" => Some(Self::Popout),
            "Protected Inline" => Some(Self::ProtectedInline),
            "Protected Popout" => Some(Self::ProtectedPopout),
            "RichTextbox" => Some(Self::RichTextbox),
            "Date" => Some(Self::Date),
            "Time" => Some(Self::Time),
            "Date Time" => Some(Self::DateTime),
            "Checkbox" => Some(Self::Checkbox),
            "Listbox" => Some(Self::Listbox),
            "Divider" => Some(Self::Divider),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inline => "Inline",
            Self::InlineUrl => "Inline URL",
            Self::Popout => "Popout",
            Self::ProtectedInline => "Protected Inline",
            Self::ProtectedPopout => "Protected Popout",
            Self::RichTextbox => "RichTextbox",
            Self::Date => "Date",
            Self::Time => "Time",
            Self::DateTime => "Date Time",
            Self::Checkbox => "Checkbox",
            Self::Listbox => "Listbox",
            Self::Divider => "Divider",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EtmTarget {
    Standard(StandardField),
    Custom(String),
    Confirm,
    OverrideUrl,
    Expiration,
    Tags,
    Special(String),
}

impl EtmTarget {
    pub fn from_storage_name(name: &str) -> Self {
        match name {
            "Title" => Self::Standard(StandardField::Title),
            "UserName" => Self::Standard(StandardField::UserName),
            "Password" => Self::Standard(StandardField::Password),
            "URL" => Self::Standard(StandardField::Url),
            "Notes" => Self::Standard(StandardField::Notes),
            "@confirm" => Self::Confirm,
            "@override" => Self::OverrideUrl,
            "@exp_date" => Self::Expiration,
            "@tags" => Self::Tags,
            special if special.starts_with('@') => Self::Special(special.to_string()),
            custom => Self::Custom(custom.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EtmField {
    pub title: String,
    pub storage_name: String,
    pub target: EtmTarget,
    pub field_type: Option<EtmFieldType>,
    pub position: i32,
    pub lines: u8,
    pub list_options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EtmTemplate {
    pub fields: Vec<EtmField>,
}

/// Parse unprotected ETM metadata from a strictly marked template entry.
pub fn parse_etm_template(entry: &Entry) -> Option<EtmTemplate> {
    if !entry.is_etm_template() {
        return None;
    }

    let mut fields = Vec::new();
    let title_counts = entry
        .custom_fields()
        .filter_map(|(_, field)| field.name().strip_prefix(ETM_TITLE_PREFIX))
        .fold(HashMap::<&str, usize>::new(), |mut counts, name| {
            *counts.entry(name).or_default() += 1;
            counts
        });
    for (_, field) in entry.custom_fields() {
        let Some(storage_name) = field.name().strip_prefix(ETM_TITLE_PREFIX) else {
            continue;
        };
        if field.value().is_protected() || title_counts.get(storage_name) != Some(&1) {
            continue;
        }

        let field_type =
            plain_metadata(entry, ETM_TYPE_PREFIX, storage_name).and_then(EtmFieldType::parse);
        let position = plain_metadata(entry, ETM_POSITION_PREFIX, storage_name)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let options = plain_metadata(entry, ETM_OPTIONS_PREFIX, storage_name).unwrap_or("");
        let lines = if matches!(
            field_type,
            Some(EtmFieldType::Inline | EtmFieldType::ProtectedInline | EtmFieldType::RichTextbox)
        ) {
            options
                .parse::<u8>()
                .ok()
                .filter(|lines| (1..=100).contains(lines))
                .unwrap_or(1)
        } else {
            1
        };
        let list_options = if field_type == Some(EtmFieldType::Listbox) {
            options
                .split(',')
                .map(str::trim)
                .filter(|option| !option.is_empty())
                .map(str::to_string)
                .collect()
        } else {
            Vec::new()
        };

        fields.push(EtmField {
            title: field.value().as_str().to_string(),
            storage_name: storage_name.to_string(),
            target: EtmTarget::from_storage_name(storage_name),
            field_type,
            position,
            lines,
            list_options,
        });
    }
    fields.sort_by_key(|field| field.position);
    Some(EtmTemplate { fields })
}

fn plain_metadata<'a>(entry: &'a Entry, prefix: &str, storage_name: &str) -> Option<&'a str> {
    let expected = format!("{prefix}{storage_name}");
    let mut matches = entry
        .custom_fields()
        .filter(|(_, field)| field.name() == expected);
    let field = matches.next()?.1;
    if matches.next().is_some() || field.value().is_protected() {
        return None;
    }
    Some(field.value().as_str())
}

pub(crate) fn parse_template_uuid(entry: &Entry) -> Option<Uuid> {
    let mut matches = entry
        .custom_fields()
        .filter(|(_, field)| field.name() == ETM_TEMPLATE_UUID);
    let field = matches.next()?.1;
    if matches.next().is_some() || field.value().is_protected() {
        return None;
    }
    Uuid::parse_str(field.value().as_str()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::core::{NodeId, ProtectedString};

    #[test]
    fn parses_strict_unprotected_metadata_in_position_order() {
        let mut entry = Entry::new(NodeId::new_uuid());
        entry.add_custom_field(ETM_TEMPLATE, ProtectedString::new_plain("1"));
        entry.add_custom_field("_etm_title_second", ProtectedString::new_plain("Second"));
        entry.add_custom_field("_etm_type_second", ProtectedString::new_plain("Listbox"));
        entry.add_custom_field("_etm_position_second", ProtectedString::new_plain("bad"));
        entry.add_custom_field(
            "_etm_options_second",
            ProtectedString::new_plain(" one, , two "),
        );
        entry.add_custom_field("_etm_title_first", ProtectedString::new_plain("First"));
        entry.add_custom_field(
            "_etm_type_first",
            ProtectedString::new_plain("Protected Inline"),
        );
        entry.add_custom_field("_etm_position_first", ProtectedString::new_plain("-1"));
        entry.add_custom_field("_etm_options_first", ProtectedString::new_plain("101"));
        entry.add_custom_field("_etm_title_third", ProtectedString::new_plain("Third"));

        let parsed = parse_etm_template(&entry).unwrap();
        assert_eq!(parsed.fields[0].storage_name, "first");
        assert_eq!(parsed.fields[0].lines, 1);
        assert_eq!(parsed.fields[1].storage_name, "second");
        assert_eq!(parsed.fields[1].position, 0);
        assert_eq!(parsed.fields[1].list_options, ["one", "two"]);
        assert_eq!(parsed.fields[2].storage_name, "third");
        assert_eq!(parsed.fields[2].position, 0);
    }

    #[test]
    fn classifies_standard_custom_and_special_targets() {
        assert_eq!(
            EtmTarget::from_storage_name("Password"),
            EtmTarget::Standard(StandardField::Password)
        );
        assert_eq!(
            EtmTarget::from_storage_name("field"),
            EtmTarget::Custom("field".into())
        );
        assert_eq!(EtmTarget::from_storage_name("@confirm"), EtmTarget::Confirm);
        assert_eq!(
            EtmTarget::from_storage_name("@override"),
            EtmTarget::OverrideUrl
        );
        assert_eq!(
            EtmTarget::from_storage_name("@exp_date"),
            EtmTarget::Expiration
        );
        assert_eq!(EtmTarget::from_storage_name("@tags"), EtmTarget::Tags);
        assert_eq!(
            EtmTarget::from_storage_name("@future"),
            EtmTarget::Special("@future".into())
        );
    }

    #[test]
    fn marker_must_be_unique_plain_and_exact() {
        for marker in [
            ProtectedString::new_plain("true"),
            ProtectedString::new_protected("1"),
        ] {
            let mut entry = Entry::new(NodeId::new_uuid());
            entry.add_custom_field(ETM_TEMPLATE, marker);
            assert!(!entry.is_etm_template());
        }

        let mut duplicate = Entry::new(NodeId::new_uuid());
        duplicate.add_custom_field(ETM_TEMPLATE, ProtectedString::new_plain("1"));
        duplicate.add_custom_field(ETM_TEMPLATE, ProtectedString::new_plain("1"));
        assert!(!duplicate.is_etm_template());
    }

    #[test]
    fn duplicate_title_declarations_are_ignored() {
        let mut entry = Entry::new(NodeId::new_uuid());
        entry.add_custom_field(ETM_TEMPLATE, ProtectedString::new_plain("1"));
        entry.add_custom_field("_etm_title_name", ProtectedString::new_plain("First"));
        entry.add_custom_field("_etm_title_name", ProtectedString::new_plain("Second"));
        entry.add_custom_field("_etm_type_name", ProtectedString::new_plain("Inline"));

        assert!(parse_etm_template(&entry).unwrap().fields.is_empty());
    }
}
