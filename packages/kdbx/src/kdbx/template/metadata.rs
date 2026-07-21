use std::collections::HashMap;

use uuid::Uuid;

use crate::model::{Entry, StandardField};

pub(super) const PREFIX: &str = "_etm_";
pub(super) const MARKER: &str = "_etm_template";
pub(super) const TEMPLATE_UUID: &str = "_etm_template_uuid";
pub(super) const TITLE_PREFIX: &str = "_etm_title_";
pub(super) const TYPE_PREFIX: &str = "_etm_type_";
pub(super) const POSITION_PREFIX: &str = "_etm_position_";
pub(super) const OPTIONS_PREFIX: &str = "_etm_options_";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldType {
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

impl FieldType {
    pub(super) fn parse(value: &str) -> Option<Self> {
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

    pub(super) const fn as_str(self) -> &'static str {
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
pub(super) enum Target {
    Standard(StandardField),
    Custom(String),
    Confirm,
    OverrideUrl,
    Expiration,
    Tags,
    Special,
}

impl Target {
    fn from_storage_name(name: &str) -> Self {
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
            special if special.starts_with('@') => Self::Special,
            custom => Self::Custom(custom.to_string()),
        }
    }
}

pub(super) struct Field {
    pub title: String,
    pub storage_name: String,
    pub target: Target,
    pub field_type: Option<FieldType>,
    pub position: i32,
    pub lines: u8,
    pub list_options: Vec<String>,
}

pub(super) fn has_marker(entry: &Entry) -> bool {
    let mut markers = entry
        .custom_fields()
        .filter(|(_, field)| field.name() == MARKER);
    let Some((_, marker)) = markers.next() else {
        return false;
    };
    markers.next().is_none() && !marker.value().is_protected() && marker.value().as_str() == "1"
}

pub(super) fn parse(entry: &Entry) -> Option<Vec<Field>> {
    if !has_marker(entry) {
        return None;
    }
    let title_counts = entry
        .custom_fields()
        .filter_map(|(_, field)| field.name().strip_prefix(TITLE_PREFIX))
        .fold(HashMap::<&str, usize>::new(), |mut counts, name| {
            *counts.entry(name).or_default() += 1;
            counts
        });
    let mut fields = Vec::new();
    for (_, field) in entry.custom_fields() {
        let Some(storage_name) = field.name().strip_prefix(TITLE_PREFIX) else {
            continue;
        };
        if field.value().is_protected() || title_counts.get(storage_name) != Some(&1) {
            continue;
        }
        let field_type = plain(entry, TYPE_PREFIX, storage_name).and_then(FieldType::parse);
        let position = plain(entry, POSITION_PREFIX, storage_name)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let options = plain(entry, OPTIONS_PREFIX, storage_name).unwrap_or("");
        let lines = if matches!(
            field_type,
            Some(FieldType::Inline | FieldType::ProtectedInline | FieldType::RichTextbox)
        ) {
            options
                .parse::<u8>()
                .ok()
                .filter(|lines| (1..=100).contains(lines))
                .unwrap_or(1)
        } else {
            1
        };

        let list_options = if field_type == Some(FieldType::Listbox) {
            options
                .split(',')
                .map(str::trim)
                .filter(|option| !option.is_empty())
                .map(str::to_string)
                .collect()
        } else {
            Default::default()
        };

        fields.push(Field {
            title: field.value().as_str().to_string(),
            storage_name: storage_name.to_string(),
            target: Target::from_storage_name(storage_name),
            field_type,
            position,
            lines,
            list_options,
        });
    }
    fields.sort_by_key(|field| field.position);
    Some(fields)
}

pub(super) fn template_uuid(entry: &Entry) -> Option<Uuid> {
    let mut matches = entry
        .custom_fields()
        .filter(|(_, field)| field.name() == TEMPLATE_UUID);
    let field = matches.next()?.1;
    if matches.next().is_some() || field.value().is_protected() {
        return None;
    }
    Uuid::parse_str(field.value().as_str()).ok()
}

fn plain<'a>(entry: &'a Entry, prefix: &str, storage_name: &str) -> Option<&'a str> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NodeId, ProtectedString};

    #[test]
    fn metadata_is_strict_and_sorted_by_position() {
        let mut entry = Entry::new(NodeId::new_uuid());
        entry.add_custom_field(MARKER, ProtectedString::new_plain("1"));
        entry.add_custom_field(
            format!("{TITLE_PREFIX}second"),
            ProtectedString::new_plain("Second"),
        );
        entry.add_custom_field(
            format!("{TYPE_PREFIX}second"),
            ProtectedString::new_plain("Listbox"),
        );
        entry.add_custom_field(
            format!("{OPTIONS_PREFIX}second"),
            ProtectedString::new_plain(" one, , two "),
        );
        entry.add_custom_field(
            format!("{TITLE_PREFIX}first"),
            ProtectedString::new_plain("First"),
        );
        entry.add_custom_field(
            format!("{TYPE_PREFIX}first"),
            ProtectedString::new_plain("Protected Inline"),
        );
        entry.add_custom_field(
            format!("{POSITION_PREFIX}first"),
            ProtectedString::new_plain("-1"),
        );
        entry.add_custom_field(
            format!("{OPTIONS_PREFIX}first"),
            ProtectedString::new_plain("101"),
        );

        let fields = parse(&entry).unwrap();
        assert_eq!(fields[0].storage_name, "first");
        assert_eq!(fields[0].lines, 1);
        assert_eq!(fields[1].storage_name, "second");
        assert_eq!(fields[1].list_options, ["one", "two"]);

        entry.add_custom_field(
            format!("{TITLE_PREFIX}first"),
            ProtectedString::new_plain("Duplicate"),
        );
        assert_eq!(
            parse(&entry)
                .unwrap()
                .into_iter()
                .map(|field| field.storage_name)
                .collect::<Vec<_>>(),
            ["second"]
        );
    }

    #[test]
    fn template_link_must_be_unique_unprotected_uuid() {
        let mut entry = Entry::new(NodeId::new_uuid());
        let id = Uuid::new_v4();
        entry.add_custom_field(TEMPLATE_UUID, ProtectedString::new_plain(&id.to_string()));
        assert_eq!(template_uuid(&entry), Some(id));
        entry.add_custom_field(TEMPLATE_UUID, ProtectedString::new_plain(&id.to_string()));
        assert_eq!(template_uuid(&entry), None);
    }
}
