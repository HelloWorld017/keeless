pub(crate) mod add_entry;
pub(crate) mod add_entry_from_template;
pub(crate) mod add_group;
pub(crate) mod create;
pub(crate) mod delete_entry;
pub(crate) mod delete_group;
pub(crate) mod delete_tag;
pub(crate) mod get_config;
pub(crate) mod get_custom_icons;
pub(crate) mod get_database_status;
pub(crate) mod get_entries;
pub(crate) mod get_entry_detail;
pub(crate) mod get_entry_templates;
pub(crate) mod get_group_entries;
pub(crate) mod get_group_hierarchy;
pub(crate) mod get_storage_descriptor;
pub(crate) mod get_tag_entries;
pub(crate) mod get_tags;
pub(crate) mod get_trash_entries;
pub(crate) mod lock;
pub(crate) mod move_entry;
pub(crate) mod move_group;
pub mod mutations;
pub(crate) mod open;
pub(crate) mod rename_group;
pub(crate) mod reveal_entry_field;
pub(crate) mod save_database;
pub(crate) mod search_entries;
pub(crate) mod set_config;
pub(crate) mod unlock;
pub(crate) mod update_entry;
pub(crate) mod update_group;
pub(crate) mod update_tag_style;

use keeless_schema::{Operation, OperationSuccess};

use crate::{KeelessCore, Result};

pub(crate) async fn execute(
    core: &mut KeelessCore,
    operation: Operation,
) -> Result<OperationSuccess> {
    match operation {
        Operation::Open(args) => open::execute(core, args).await,
        Operation::Create(args) => create::execute(core, args).await,
        Operation::Unlock(args) => unlock::execute(core, args).await,
        Operation::Lock(args) => lock::execute(core, args),
        Operation::GetDatabaseStatus(args) => get_database_status::execute(core, args),
        Operation::GetStorageDescriptor(args) => get_storage_descriptor::execute(core, args),
        Operation::GetConfig(args) => get_config::execute(core, args),
        Operation::SetConfig(args) => set_config::execute(core, args).await,
        Operation::GetEntries(args) => get_entries::execute(core, args),
        Operation::SearchEntries(args) => search_entries::execute(core, args),
        Operation::GetGroupHierarchy(args) => get_group_hierarchy::execute(core, args),
        Operation::GetGroupEntries(args) => get_group_entries::execute(core, args),
        Operation::GetTagEntries(args) => get_tag_entries::execute(core, args),
        Operation::GetTrashEntries(args) => get_trash_entries::execute(core, args),
        Operation::GetTags(args) => get_tags::execute(core, args),
        Operation::GetEntryDetail(args) => get_entry_detail::execute(core, args),
        Operation::UpdateEntry(args) => update_entry::execute(core, args).await,
        Operation::DeleteEntry(args) => delete_entry::execute(core, args).await,
        Operation::SaveDatabase(args) => save_database::execute(core, args).await,
        Operation::GetCustomIcons(args) => get_custom_icons::execute(core, args),
        Operation::GetEntryTemplates(args) => get_entry_templates::execute(core, args),
        Operation::MoveGroup(args) => move_group::execute(core, args).await,
        Operation::MoveEntry(args) => move_entry::execute(core, args).await,
        Operation::AddEntry(args) => add_entry::execute(core, args).await,
        Operation::AddEntryFromTemplate(args) => add_entry_from_template::execute(core, args).await,
        Operation::AddGroup(args) => add_group::execute(core, args).await,
        Operation::DeleteGroup(args) => delete_group::execute(core, args).await,
        Operation::RenameGroup(args) => rename_group::execute(core, args).await,
        Operation::UpdateGroup(args) => update_group::execute(core, args).await,
        Operation::UpdateTagStyle(args) => update_tag_style::execute(core, args).await,
        Operation::DeleteTag(args) => delete_tag::execute(core, args).await,
        Operation::RevealEntryField(args) => reveal_entry_field::execute(core, args).await,
    }
}
