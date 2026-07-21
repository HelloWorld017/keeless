//! Template engine
//!
//! Template definitions and inference for entry types (Login, Credit Card, etc.)

use crate::model::core::{NodeId, ProtectedString};
use crate::model::entry::Entry;
use crate::model::meta::icon::{IconImage, IconImageStandard};

/// Template field definition
#[derive(Debug, Clone)]
pub struct TemplateField {
    pub name: String,
    pub field_type: TemplateFieldType,
    pub is_protected: bool,
}

/// Template field types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateFieldType {
    Text,
    Password,
    Url,
    Email,
    Number,
    Date,
    Toggle,
    List,
    UserName,
}

/// A complete template definition
#[derive(Debug, Clone)]
pub struct Template {
    pub uuid: uuid::Uuid,
    pub name: String,
    pub title: String,
    pub icon_id: u32,
    pub fields: Vec<TemplateField>,
}

impl Template {
    pub fn new(name: &str, icon_id: u32, fields: Vec<TemplateField>) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4(),
            name: name.to_string(),
            title: name.to_string(),
            icon_id,
            fields,
        }
    }

    /// Check if a set of field names matches this template
    pub fn matches_fields(&self, field_names: &[&str]) -> bool {
        let required: Vec<&str> = self
            .fields
            .iter()
            .map(|f| f.name.as_str())
            .filter(|n| !matches!(*n, "Title" | "UserName" | "Password" | "URL" | "Notes"))
            .collect();

        if required.is_empty() {
            return false;
        }

        let matches_count = required
            .iter()
            .filter(|r| field_names.iter().any(|f| f.eq_ignore_ascii_case(r)))
            .count();

        // Match if at least half of the required fields are present
        matches_count * 2 >= required.len()
    }

    pub fn into_entry(self) -> Entry {
        let mut entry = Entry::new(NodeId::from_uuid(self.uuid));
        entry.set_title(ProtectedString::new_plain(&self.title));
        entry.icon = IconImage::Standard(IconImageStandard::new(self.icon_id));
        entry.is_template = true;

        for field in self.fields {
            let value = || {
                if field.is_protected {
                    ProtectedString::new_protected("")
                } else {
                    ProtectedString::new_plain("")
                }
            };
            match field.name.as_str() {
                "Title" => {
                    let title = entry.title().as_str().to_string();
                    entry.set_title(if field.is_protected {
                        ProtectedString::new_protected(&title)
                    } else {
                        ProtectedString::new_plain(&title)
                    });
                }
                "UserName" => entry.set_username(value()),
                "Password" => entry.set_password(value()),
                "URL" => entry.set_url(value()),
                "Notes" => entry.set_notes(value()),
                _ => {
                    entry.add_custom_field(field.name, value());
                }
            }
        }

        entry
    }
}

/// Built-in template definitions
pub fn get_builtin_templates() -> Vec<Template> {
    vec![
        Template::new(
            "General",
            0,
            vec![
                TemplateField {
                    name: "UserName".into(),
                    field_type: TemplateFieldType::UserName,
                    is_protected: false,
                },
                TemplateField {
                    name: "Password".into(),
                    field_type: TemplateFieldType::Password,
                    is_protected: true,
                },
                TemplateField {
                    name: "URL".into(),
                    field_type: TemplateFieldType::Url,
                    is_protected: false,
                },
                TemplateField {
                    name: "Notes".into(),
                    field_type: TemplateFieldType::Text,
                    is_protected: false,
                },
            ],
        ),
        Template::new(
            "Credit Card",
            37,
            vec![
                TemplateField {
                    name: "UserName".into(),
                    field_type: TemplateFieldType::UserName,
                    is_protected: false,
                },
                TemplateField {
                    name: "Password".into(),
                    field_type: TemplateFieldType::Password,
                    is_protected: true,
                },
                TemplateField {
                    name: "Card Number".into(),
                    field_type: TemplateFieldType::Number,
                    is_protected: true,
                },
                TemplateField {
                    name: "Cardholder Name".into(),
                    field_type: TemplateFieldType::Text,
                    is_protected: false,
                },
                TemplateField {
                    name: "CVV".into(),
                    field_type: TemplateFieldType::Password,
                    is_protected: true,
                },
                TemplateField {
                    name: "Expiry Date".into(),
                    field_type: TemplateFieldType::Date,
                    is_protected: false,
                },
                TemplateField {
                    name: "PIN".into(),
                    field_type: TemplateFieldType::Password,
                    is_protected: true,
                },
            ],
        ),
        Template::new(
            "Email Account",
            31,
            vec![
                TemplateField {
                    name: "Email".into(),
                    field_type: TemplateFieldType::Email,
                    is_protected: false,
                },
                TemplateField {
                    name: "UserName".into(),
                    field_type: TemplateFieldType::UserName,
                    is_protected: false,
                },
                TemplateField {
                    name: "Password".into(),
                    field_type: TemplateFieldType::Password,
                    is_protected: true,
                },
                TemplateField {
                    name: "SMTP Server".into(),
                    field_type: TemplateFieldType::Text,
                    is_protected: false,
                },
                TemplateField {
                    name: "IMAP Server".into(),
                    field_type: TemplateFieldType::Text,
                    is_protected: false,
                },
            ],
        ),
        Template::new(
            "Wireless Router",
            48,
            vec![
                TemplateField {
                    name: "UserName".into(),
                    field_type: TemplateFieldType::UserName,
                    is_protected: false,
                },
                TemplateField {
                    name: "Password".into(),
                    field_type: TemplateFieldType::Password,
                    is_protected: true,
                },
                TemplateField {
                    name: "SSID".into(),
                    field_type: TemplateFieldType::Text,
                    is_protected: false,
                },
                TemplateField {
                    name: "Wireless Security".into(),
                    field_type: TemplateFieldType::List,
                    is_protected: false,
                },
            ],
        ),
        Template::new(
            "Bank Account",
            38,
            vec![
                TemplateField {
                    name: "Bank Name".into(),
                    field_type: TemplateFieldType::Text,
                    is_protected: false,
                },
                TemplateField {
                    name: "Account Number".into(),
                    field_type: TemplateFieldType::Number,
                    is_protected: true,
                },
                TemplateField {
                    name: "Routing Number".into(),
                    field_type: TemplateFieldType::Number,
                    is_protected: false,
                },
                TemplateField {
                    name: "SWIFT Code".into(),
                    field_type: TemplateFieldType::Text,
                    is_protected: false,
                },
                TemplateField {
                    name: "IBAN".into(),
                    field_type: TemplateFieldType::Text,
                    is_protected: true,
                },
            ],
        ),
        Template::new(
            "Secure Note",
            0,
            vec![TemplateField {
                name: "Notes".into(),
                field_type: TemplateFieldType::Text,
                is_protected: false,
            }],
        ),
        Template::new(
            "SSH Key",
            23,
            vec![
                TemplateField {
                    name: "UserName".into(),
                    field_type: TemplateFieldType::UserName,
                    is_protected: false,
                },
                TemplateField {
                    name: "Private Key".into(),
                    field_type: TemplateFieldType::Text,
                    is_protected: true,
                },
                TemplateField {
                    name: "Public Key".into(),
                    field_type: TemplateFieldType::Text,
                    is_protected: false,
                },
                TemplateField {
                    name: "Passphrase".into(),
                    field_type: TemplateFieldType::Password,
                    is_protected: true,
                },
                TemplateField {
                    name: "Host".into(),
                    field_type: TemplateFieldType::Text,
                    is_protected: false,
                },
            ],
        ),
        Template::new(
            "Membership",
            46,
            vec![
                TemplateField {
                    name: "UserName".into(),
                    field_type: TemplateFieldType::UserName,
                    is_protected: false,
                },
                TemplateField {
                    name: "Password".into(),
                    field_type: TemplateFieldType::Password,
                    is_protected: true,
                },
                TemplateField {
                    name: "Membership Number".into(),
                    field_type: TemplateFieldType::Number,
                    is_protected: true,
                },
                TemplateField {
                    name: "Expiry Date".into(),
                    field_type: TemplateFieldType::Date,
                    is_protected: false,
                },
            ],
        ),
    ]
}

/// Infer the most likely template from an entry's custom fields.
/// Returns the template name, or "General" if no match found.
pub fn infer_template(field_names: &[&str]) -> String {
    let templates = get_builtin_templates();
    let mut best_match: Option<&Template> = None;
    let mut best_score = 0;

    for t in &templates {
        let _score = t.matches_fields(field_names);
        let match_count = count_matches(t, field_names);
        if match_count > best_score {
            best_score = match_count;
            best_match = Some(t);
        }
    }

    best_match
        .map(|t| t.name.clone())
        .unwrap_or_else(|| "General".to_string())
}

fn count_matches(template: &Template, field_names: &[&str]) -> usize {
    template
        .fields
        .iter()
        .filter(|f| field_names.iter().any(|n| n.eq_ignore_ascii_case(&f.name)))
        .count()
}

/// Check if an entry looks like a template entry
pub fn is_template_entry(title: &str, _custom_field_names: &[&str]) -> bool {
    // An entry is likely a template if its title matches a known template name
    let templates = get_builtin_templates();
    templates
        .iter()
        .any(|t| t.title.eq_ignore_ascii_case(title))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_templates() {
        let templates = get_builtin_templates();
        assert!(!templates.is_empty());
        assert!(templates.iter().any(|t| t.name == "General"));
        assert!(templates.iter().any(|t| t.name == "Credit Card"));
    }

    #[test]
    fn builtin_template_converts_to_generic_entry() {
        let template = get_builtin_templates()
            .into_iter()
            .find(|template| template.name == "Credit Card")
            .unwrap();
        let entry = template.into_entry();

        assert_eq!(entry.title(), "Credit Card");
        assert!(entry.is_template);
        assert_eq!(entry.custom_fields().count(), 5);
        assert!(entry
            .custom_fields()
            .find(|(_, field)| field.name == "Card Number")
            .unwrap()
            .1
            .value
            .is_protected());
        assert!(matches!(
            entry.icon,
            IconImage::Standard(IconImageStandard { icon_id: 37 })
        ));
    }

    #[test]
    fn test_infer_template_credit_card() {
        let fields = ["Card Number", "Cardholder Name", "CVV"];
        let result = infer_template(&fields);
        assert_eq!(result, "Credit Card");
    }

    #[test]
    fn test_infer_template_bank_account() {
        let fields = ["Bank Name", "Account Number", "Routing Number"];
        let result = infer_template(&fields);
        assert_eq!(result, "Bank Account");
    }

    #[test]
    fn test_infer_template_general_fallback() {
        let fields: [&str; 0] = [];
        let result = infer_template(&fields);
        assert_eq!(result, "General");
    }

    #[test]
    fn test_template_matches_fields() {
        let t = Template::new(
            "Test",
            0,
            vec![
                TemplateField {
                    name: "A".into(),
                    field_type: TemplateFieldType::Text,
                    is_protected: false,
                },
                TemplateField {
                    name: "B".into(),
                    field_type: TemplateFieldType::Text,
                    is_protected: false,
                },
            ],
        );
        assert!(t.matches_fields(&["A", "B"]));
        assert!(t.matches_fields(&["A"]));
        assert!(!t.matches_fields(&["X"]));
    }

    #[test]
    fn test_is_template_entry() {
        assert!(is_template_entry("Credit Card", &[]));
        assert!(is_template_entry("General", &[]));
        assert!(!is_template_entry("My Login", &[]));
    }
}
