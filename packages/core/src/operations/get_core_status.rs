use keeless_schema::{CoreStatusResult, GetCoreStatusArgs, OperationSuccess};

use crate::{KeelessCore, Result};

pub(super) fn execute(
    core: &mut KeelessCore,
    _args: GetCoreStatusArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetCoreStatus(CoreStatusResult {
        database: super::get_database_status::run(core),
    }))
}
