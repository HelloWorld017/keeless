pub(crate) mod assert_passkey;
pub(crate) mod create;
pub(crate) mod delete_recent_database;
pub(crate) mod get_config;
pub(crate) mod get_core_status;
pub(crate) mod get_custom_icons;
pub(crate) mod get_database_status;
pub(crate) mod get_entries;
pub(crate) mod get_entry_detail;
pub(crate) mod get_entry_templates;
pub(crate) mod get_entry_totp;
pub(crate) mod get_group_entries;
pub(crate) mod get_group_hierarchy;
pub(crate) mod get_passkeys;
pub(crate) mod get_recent_databases;
pub(crate) mod get_storage_provider;
pub(crate) mod get_tag_entries;
pub(crate) mod get_tags;
pub(crate) mod get_trash_entries;
pub(crate) mod lock;
pub(crate) mod merge_transferred_database;
pub(crate) mod mutations;
pub(crate) mod open;
pub(crate) mod prepare_database_export;
pub(crate) mod prepare_entry_attachment_download;
pub(crate) mod recent;
pub(crate) mod reveal_entry_fields;
pub(crate) mod save_database;
pub(crate) mod search_entries;
pub(crate) mod search_fuzzy;
pub(crate) mod set_config;
pub(crate) mod unlock;
pub(crate) mod upgrade;

use keeless_schema::{Operation, OperationSuccess};

use crate::{KeelessCore, Result};

pub(crate) async fn execute(
    core: &mut KeelessCore,
    operation: Operation,
) -> Result<OperationSuccess> {
    match operation {
        Operation::Open(args) => open::execute(core, args).await,
        Operation::GetRecentDatabases(args) => get_recent_databases::execute(core, args).await,
        Operation::DeleteRecentDatabase(args) => delete_recent_database::execute(core, args).await,
        Operation::Create(args) => create::execute(core, args).await,
        Operation::Unlock(args) => unlock::execute(core, args).await,
        Operation::Lock(args) => lock::execute(core, args).await,
        Operation::GetCoreStatus(args) => get_core_status::execute(core, args),
        Operation::Upgrade(args) => upgrade::execute(core, args).await,
        Operation::GetDatabaseStatus(args) => get_database_status::execute(core, args),
        Operation::GetStorageProvider(args) => get_storage_provider::execute(core, args),
        Operation::GetConfig(args) => get_config::execute(core, args),
        Operation::SetConfig(args) => set_config::execute(core, args).await,
        Operation::GetEntries(args) => get_entries::execute(core, args),
        Operation::SearchEntries(args) => search_entries::execute(core, args),
        Operation::SearchFuzzy(args) => search_fuzzy::execute(core, args),
        Operation::GetGroupHierarchy(args) => get_group_hierarchy::execute(core, args),
        Operation::GetGroupEntries(args) => get_group_entries::execute(core, args),
        Operation::GetTagEntries(args) => get_tag_entries::execute(core, args),
        Operation::GetTrashEntries(args) => get_trash_entries::execute(core, args),
        Operation::GetTags(args) => get_tags::execute(core, args),
        Operation::GetEntryDetail(args) => get_entry_detail::execute(core, args),
        Operation::GetEntryTotp(args) => get_entry_totp::execute(core, args).await,
        Operation::UpdateEntry(args) => mutations::update_entry::execute(core, args).await,
        Operation::PrepareEntryAttachmentDownload(args) => {
            prepare_entry_attachment_download::execute(core, args)
        }
        Operation::DeleteEntry(args) => mutations::delete_entry::execute(core, args).await,
        Operation::EmptyRecycleBin(args) => mutations::empty_recycle_bin::execute(core, args).await,
        Operation::SaveDatabase(args) => save_database::execute(core, args).await,
        Operation::PrepareDatabaseExport(args) => {
            prepare_database_export::execute(core, args).await
        }
        Operation::MergeTransferredDatabase(args) => {
            merge_transferred_database::execute(core, args).await
        }
        Operation::GetCustomIcons(args) => get_custom_icons::execute(core, args),
        Operation::GetEntryTemplates(args) => get_entry_templates::execute(core, args),
        Operation::MoveGroup(args) => mutations::move_group::execute(core, args).await,
        Operation::MoveEntry(args) => mutations::move_entry::execute(core, args).await,
        Operation::AddEntry(args) => mutations::add_entry::execute(core, args).await,
        Operation::AddEntryFromTemplate(args) => {
            mutations::add_entry_from_template::execute(core, args).await
        }
        Operation::AddGroup(args) => mutations::add_group::execute(core, args).await,
        Operation::DeleteGroup(args) => mutations::delete_group::execute(core, args).await,
        Operation::RenameGroup(args) => mutations::rename_group::execute(core, args).await,
        Operation::UpdateGroup(args) => mutations::update_group::execute(core, args).await,
        Operation::UpdateTagStyle(args) => mutations::update_tag_style::execute(core, args).await,
        Operation::DeleteTag(args) => mutations::delete_tag::execute(core, args).await,
        Operation::RevealEntryFields(args) => reveal_entry_fields::execute(core, args).await,
        Operation::GetPasskeys(args) => get_passkeys::execute(core, args).await,
        Operation::RegisterPasskey(args) => mutations::register_passkey::execute(core, args).await,
        Operation::AssertPasskey(args) => assert_passkey::execute(core, args).await,
    }
}
