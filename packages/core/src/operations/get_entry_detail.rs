use keeless_kdbx::kdbx::template::{self, FieldControl as KdbxControl, LayoutTarget as KdbxTarget};
use keeless_schema::{
    EntryDetailResult, EntryLayout, EntryLayoutItem, FieldControl, GetEntryDetailArgs,
    LayoutTarget, OperationSuccess,
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
        detail
            .fields
            .retain(|field| !template::is_internal_field(&field.name));
        detail.layout = template::resolve_layout(database, entry).map(|layout| EntryLayout {
            template_id: layout.template_id,
            items: layout
                .items
                .into_iter()
                .map(|item| EntryLayoutItem {
                    label: item.label,
                    target: match item.target {
                        KdbxTarget::Field {
                            field_id,
                            field_name,
                        } => LayoutTarget::Field {
                            field_id,
                            field_name,
                        },
                        KdbxTarget::PasswordConfirmation { password_field_id } => {
                            LayoutTarget::PasswordConfirmation { password_field_id }
                        }
                        KdbxTarget::OverrideUrl => LayoutTarget::OverrideUrl,
                        KdbxTarget::Expiry => LayoutTarget::Expiry,
                        KdbxTarget::Tags => LayoutTarget::Tags,
                        KdbxTarget::Divider => LayoutTarget::Divider,
                    },
                    control: match item.control {
                        KdbxControl::Text { protected, lines } => {
                            FieldControl::Text { protected, lines }
                        }
                        KdbxControl::Url => FieldControl::Url,
                        KdbxControl::Popout { protected } => FieldControl::Popout { protected },
                        KdbxControl::RichText { lines } => FieldControl::RichText { lines },
                        KdbxControl::Date => FieldControl::Date,
                        KdbxControl::Time => FieldControl::Time,
                        KdbxControl::DateTime => FieldControl::DateTime,
                        KdbxControl::Checkbox => FieldControl::Checkbox,
                        KdbxControl::Select { options } => FieldControl::Select { options },
                        KdbxControl::Divider => FieldControl::Divider,
                    },
                })
                .collect(),
        });
        detail
    };
    core.touch_activity();
    Ok(result)
}

pub(super) fn execute(
    core: &mut KeelessCore,
    args: GetEntryDetailArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetEntryDetail(Box::new(run(core, args)?)))
}
