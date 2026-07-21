use crate::model::{Database, Entry, EntryFieldId, StandardField};

use super::entry::resolve;
use super::metadata::{self, FieldType, Target};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryLayout {
    pub template_id: String,
    pub items: Vec<EntryLayoutItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryLayoutItem {
    pub label: String,
    pub target: LayoutTarget,
    pub control: FieldControl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutTarget {
    Field {
        field_id: Option<String>,
        field_name: String,
    },
    PasswordConfirmation {
        password_field_id: String,
    },
    OverrideUrl,
    Expiry,
    Tags,
    Divider,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldControl {
    Text { protected: bool, lines: u8 },
    Url,
    Popout { protected: bool },
    RichText { lines: u8 },
    Date,
    Time,
    DateTime,
    Checkbox,
    Select { options: Vec<String> },
    Divider,
}

pub fn resolve_layout(database: &Database, entry: &Entry) -> Option<EntryLayout> {
    let template = resolve(database, entry)?;
    let items = metadata::parse(template)?
        .into_iter()
        .filter_map(|field| {
            let field_type = field.field_type?;
            let control = match field_type {
                FieldType::Inline => FieldControl::Text {
                    protected: false,
                    lines: field.lines,
                },
                FieldType::ProtectedInline => FieldControl::Text {
                    protected: true,
                    lines: field.lines,
                },
                FieldType::InlineUrl => FieldControl::Url,
                FieldType::Popout => FieldControl::Popout { protected: false },
                FieldType::ProtectedPopout => FieldControl::Popout { protected: true },
                FieldType::RichTextbox => FieldControl::RichText { lines: field.lines },
                FieldType::Date => FieldControl::Date,
                FieldType::Time => FieldControl::Time,
                FieldType::DateTime => FieldControl::DateTime,
                FieldType::Checkbox => FieldControl::Checkbox,
                FieldType::Listbox => FieldControl::Select {
                    options: field.list_options,
                },
                FieldType::Divider => FieldControl::Divider,
            };
            let target = if field_type == FieldType::Divider {
                LayoutTarget::Divider
            } else {
                match field.target {
                    Target::Standard(standard) => LayoutTarget::Field {
                        field_id: Some(EntryFieldId::Standard(standard).to_string()),
                        field_name: field.storage_name.clone(),
                    },
                    Target::Custom(name) => LayoutTarget::Field {
                        field_id: unique_custom_field_id(entry, &name)?,
                        field_name: name,
                    },
                    Target::Confirm => LayoutTarget::PasswordConfirmation {
                        password_field_id: EntryFieldId::Standard(StandardField::Password)
                            .to_string(),
                    },
                    Target::OverrideUrl => LayoutTarget::OverrideUrl,
                    Target::Expiration => LayoutTarget::Expiry,
                    Target::Tags => LayoutTarget::Tags,
                    Target::Special => return None,
                }
            };
            Some(EntryLayoutItem {
                label: if field.title.is_empty() {
                    field.storage_name
                } else {
                    field.title
                },
                target,
                control,
            })
        })
        .collect();
    Some(EntryLayout {
        template_id: template.id.as_uuid()?.hyphenated().to_string(),
        items,
    })
}

fn unique_custom_field_id(entry: &Entry, name: &str) -> Option<Option<String>> {
    let mut matches = entry
        .custom_fields()
        .filter(|(_, field)| field.name() == name);
    let Some((id, _)) = matches.next() else {
        return Some(None);
    };
    matches.next().is_none().then(|| Some(id.to_string()))
}
