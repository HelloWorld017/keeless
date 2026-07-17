pub(crate) mod get_config;
pub(crate) mod get_database_status;
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
        Operation::Unlock(args) => unlock::execute(core, args).await,
        Operation::Lock(args) => lock::execute(core, args),
        Operation::GetDatabaseStatus(args) => get_database_status::execute(core, args),
        Operation::GetConfig(args) => get_config::execute(core, args),
        Operation::SetConfig(args) => set_config::execute(core, args).await,
    }
}
