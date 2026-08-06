use keeless_kdbx::kdbx::template::{self, FieldControl as KdbxControl, LayoutTarget as KdbxTarget};
use keeless_kdbx::{
    Database, Entry, StandardField, is_keepass_timeotp_field, is_keepass_timeotp_secret_field,
};
use keeless_schema::{EntryFieldInformation, EntryFieldKind, FieldControl};

/// Core presentation policy for fields that are not ordinary user fields.
pub(super) fn is_internal_field(name: &str) -> bool {
    template::is_template_field(name) || is_keepass_timeotp_field(name)
}

pub(super) fn build_entry_fields(database: &Database, entry: &Entry) -> Vec<EntryFieldInformation> {
    let mut fields = entry
        .fields()
        .map(|(id, field)| {
            let is_protected = field.value().is_protected();
            let kind = match field.standard() {
                Some(StandardField::Title) => EntryFieldKind::Title,
                Some(StandardField::UserName) => EntryFieldKind::UserName,
                Some(StandardField::Password) => EntryFieldKind::Password,
                Some(StandardField::Url) => EntryFieldKind::Url,
                Some(StandardField::Notes) => EntryFieldKind::Notes,
                None => EntryFieldKind::Custom,
            };
            EntryFieldInformation::Field {
                order: 0,
                field_id: Some(id.to_string()),
                kind,
                name: field.name().to_string(),
                label: field.name().to_string(),
                value: (!is_protected).then(|| field.value().as_str().to_string()),
                is_protected,
                is_internal: is_internal_field(field.name()),
                control: None,
            }
        })
        .collect::<Vec<_>>();
    let stored_field_count = fields.len();
    let mut layout_order = Vec::new();

    if let Some(layout) = template::resolve_layout(database, entry) {
        for item in layout.items {
            let control = convert_control(item.control);
            let index = match item.target {
                KdbxTarget::Field {
                    field_id,
                    field_name,
                } => {
                    let matching_index = field_id.as_deref().and_then(|field_id| {
                        fields.iter().position(|field| {
                            matches!(
                                field,
                                EntryFieldInformation::Field {
                                    field_id: Some(candidate),
                                    ..
                                } if candidate == field_id
                            )
                        })
                    });
                    if let Some(index) = matching_index {
                        if let EntryFieldInformation::Field {
                            label,
                            control: slot,
                            ..
                        } = &mut fields[index]
                        {
                            *label = item.label;
                            *slot = Some(control);
                        }
                        Some(index)
                    } else if field_id.is_none()
                        && !fields.iter().any(|field| {
                            matches!(
                                field,
                                EntryFieldInformation::Field { name, .. } if name == &field_name
                            )
                        })
                    {
                        let is_protected = matches!(
                            control,
                            FieldControl::Text {
                                protected: true,
                                ..
                            } | FieldControl::Popout { protected: true }
                        );
                        fields.push(EntryFieldInformation::Field {
                            order: 0,
                            field_id: None,
                            kind: EntryFieldKind::Custom,
                            name: field_name,
                            label: item.label,
                            value: Some(String::new()),
                            is_protected,
                            is_internal: false,
                            control: Some(control),
                        });
                        Some(fields.len() - 1)
                    } else {
                        None
                    }
                }
                KdbxTarget::PasswordConfirmation { password_field_id } => {
                    fields.push(EntryFieldInformation::PasswordConfirmation {
                        order: 0,
                        label: item.label,
                        password_field_id,
                        control,
                    });
                    Some(fields.len() - 1)
                }
                KdbxTarget::OverrideUrl => {
                    fields.push(EntryFieldInformation::OverrideUrl {
                        order: 0,
                        label: item.label,
                        control,
                    });
                    Some(fields.len() - 1)
                }
                KdbxTarget::Expiry => {
                    fields.push(EntryFieldInformation::Expiry {
                        order: 0,
                        label: item.label,
                        control,
                    });
                    Some(fields.len() - 1)
                }
                KdbxTarget::Tags => {
                    fields.push(EntryFieldInformation::Tags {
                        order: 0,
                        label: item.label,
                        control,
                    });
                    Some(fields.len() - 1)
                }
                KdbxTarget::Divider => {
                    fields.push(EntryFieldInformation::Divider {
                        order: 0,
                        label: item.label,
                    });
                    Some(fields.len() - 1)
                }
            };
            if let Some(index) = index
                && !is_standard_field(&fields[index])
                && !layout_order.contains(&index)
            {
                layout_order.push(index);
            }
        }
    }

    if entry
        .custom_fields()
        .any(|(_, field)| is_keepass_timeotp_secret_field(field.name()))
    {
        fields.push(EntryFieldInformation::TimeOtp {
            order: 0,
            label: "OTP".into(),
        });
    }

    let mut display_order = Vec::with_capacity(fields.len());
    for kind in [
        EntryFieldKind::Title,
        EntryFieldKind::UserName,
        EntryFieldKind::Password,
        EntryFieldKind::Url,
        EntryFieldKind::Notes,
    ] {
        if let Some(index) = fields.iter().position(|field| {
            matches!(field, EntryFieldInformation::Field { kind: candidate, .. } if candidate == &kind)
        }) {
            display_order.push(index);
        }
    }
    display_order.extend(layout_order);
    for index in 0..stored_field_count {
        if !display_order.contains(&index) {
            display_order.push(index);
        }
    }
    for index in stored_field_count..fields.len() {
        if !display_order.contains(&index) {
            display_order.push(index);
        }
    }
    for (order, index) in display_order.into_iter().enumerate() {
        set_order(&mut fields[index], order as u64);
    }
    fields
}

fn is_standard_field(field: &EntryFieldInformation) -> bool {
    matches!(
        field,
        EntryFieldInformation::Field {
            kind: EntryFieldKind::Title
                | EntryFieldKind::UserName
                | EntryFieldKind::Password
                | EntryFieldKind::Url
                | EntryFieldKind::Notes,
            ..
        }
    )
}

fn set_order(field: &mut EntryFieldInformation, value: u64) {
    match field {
        EntryFieldInformation::Field { order, .. }
        | EntryFieldInformation::TimeOtp { order, .. }
        | EntryFieldInformation::PasswordConfirmation { order, .. }
        | EntryFieldInformation::OverrideUrl { order, .. }
        | EntryFieldInformation::Expiry { order, .. }
        | EntryFieldInformation::Tags { order, .. }
        | EntryFieldInformation::Divider { order, .. } => *order = value,
    }
}

fn convert_control(control: KdbxControl) -> FieldControl {
    match control {
        KdbxControl::Text { protected, lines } => FieldControl::Text { protected, lines },
        KdbxControl::Url => FieldControl::Url,
        KdbxControl::Popout { protected } => FieldControl::Popout { protected },
        KdbxControl::RichText { lines } => FieldControl::RichText { lines },
        KdbxControl::Date => FieldControl::Date,
        KdbxControl::Time => FieldControl::Time,
        KdbxControl::DateTime => FieldControl::DateTime,
        KdbxControl::Checkbox => FieldControl::Checkbox,
        KdbxControl::Select { options } => FieldControl::Select { options },
        KdbxControl::Divider => FieldControl::Divider,
    }
}

#[cfg(test)]
mod tests {
    use super::is_internal_field;

    #[test]
    fn template_metadata_is_initially_internal() {
        assert!(is_internal_field("_etm_template_uuid"));
        assert!(is_internal_field("TimeOtp-Secret-Base32"));
        assert!(is_internal_field("TimeOtp-Extension"));
        assert!(!is_internal_field("Custom"));
    }
}
