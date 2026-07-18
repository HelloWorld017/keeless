pub(crate) mod create;
mod database_dto;
pub(crate) mod get_config;
pub(crate) mod get_custom_icons;
pub(crate) mod get_database_status;
pub(crate) mod get_entries;
pub(crate) mod get_entry_detail;
pub(crate) mod get_group_entries;
pub(crate) mod get_group_hierarchy;
pub(crate) mod get_tags;
pub(crate) mod lock;
pub(crate) mod open;
pub(crate) mod set_config;
pub(crate) mod unlock;

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
        Operation::GetConfig(args) => get_config::execute(core, args),
        Operation::SetConfig(args) => set_config::execute(core, args).await,
        Operation::GetEntries(args) => get_entries::execute(core, args),
        Operation::GetGroupHierarchy(args) => get_group_hierarchy::execute(core, args),
        Operation::GetGroupEntries(args) => get_group_entries::execute(core, args),
        Operation::GetTags(args) => get_tags::execute(core, args),
        Operation::GetEntryDetail(args) => get_entry_detail::execute(core, args),
        Operation::GetCustomIcons(args) => get_custom_icons::execute(core, args),
    }
}
