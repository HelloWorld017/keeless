use keeless_kdbx::kdbx::template::{self, FieldControl as KdbxControl, LayoutTarget as KdbxTarget};
use keeless_schema::{
    EntryDetailResult, EntryFieldInformation, EntryFieldKind, FieldControl, GetEntryDetailArgs,
    OperationSuccess,
};

use crate::model::{entry_detail, parse_node_id};
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore, args: GetEntryDetailArgs) -> Result<EntryDetailResult> {
    core.enforce_auto_lock();
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let entry_id = parse_node_id(args.entry_id)?;
    let result = {
        let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
        let database = handle.database();
        let entry = database
            .get_entry(&entry_id)
            .ok_or(CoreError::EntryNotFound)?;
        let mut detail = entry_detail(entry);
        detail.is_template = template::is_template(database, &entry_id);
        let stored_field_count = detail.fields.len();
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
                            detail.fields.iter().position(|field| {
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
                            } = &mut detail.fields[index]
                            {
                                *label = item.label;
                                *slot = Some(control);
                            }
                            Some(index)
                        } else if field_id.is_none()
                            && !detail.fields.iter().any(|field| {
                                matches!(
                                    field,
                                    EntryFieldInformation::Field { name, .. }
                                        if name == &field_name
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
                            detail.fields.push(EntryFieldInformation::Field {
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
                            Some(detail.fields.len() - 1)
                        } else {
                            None
                        }
                    }
                    KdbxTarget::PasswordConfirmation { password_field_id } => {
                        detail
                            .fields
                            .push(EntryFieldInformation::PasswordConfirmation {
                                order: 0,
                                label: item.label,
                                password_field_id,
                                control,
                            });
                        Some(detail.fields.len() - 1)
                    }
                    KdbxTarget::OverrideUrl => {
                        detail.fields.push(EntryFieldInformation::OverrideUrl {
                            order: 0,
                            label: item.label,
                            control,
                        });
                        Some(detail.fields.len() - 1)
                    }
                    KdbxTarget::Expiry => {
                        detail.fields.push(EntryFieldInformation::Expiry {
                            order: 0,
                            label: item.label,
                            control,
                        });
                        Some(detail.fields.len() - 1)
                    }
                    KdbxTarget::Tags => {
                        detail.fields.push(EntryFieldInformation::Tags {
                            order: 0,
                            label: item.label,
                            control,
                        });
                        Some(detail.fields.len() - 1)
                    }
                    KdbxTarget::Divider => {
                        detail.fields.push(EntryFieldInformation::Divider {
                            order: 0,
                            label: item.label,
                        });
                        Some(detail.fields.len() - 1)
                    }
                };
                if let Some(index) = index
                    && !is_standard_field(&detail.fields[index])
                    && !layout_order.contains(&index)
                {
                    layout_order.push(index);
                }
            }
        }

        let mut display_order = Vec::with_capacity(detail.fields.len());
        for kind in [
            EntryFieldKind::Title,
            EntryFieldKind::UserName,
            EntryFieldKind::Password,
            EntryFieldKind::Url,
            EntryFieldKind::Notes,
        ] {
            if let Some(index) = detail.fields.iter().position(|field| {
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
        for index in stored_field_count..detail.fields.len() {
            if !display_order.contains(&index) {
                display_order.push(index);
            }
        }
        for (order, index) in display_order.into_iter().enumerate() {
            set_order(&mut detail.fields[index], order as u64);
        }
        detail
    };
    core.touch_activity();
    Ok(result)
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

pub(super) fn execute(
    core: &mut KeelessCore,
    args: GetEntryDetailArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetEntryDetail(Box::new(run(core, args)?)))
}
