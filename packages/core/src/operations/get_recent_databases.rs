use keeless_schema::{GetRecentDatabasesArgs, OperationSuccess, RecentDatabasesResult};

use crate::{KeelessCore, Result};

pub(super) async fn execute(
    core: &mut KeelessCore,
    _args: GetRecentDatabasesArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetRecentDatabases(
        RecentDatabasesResult {
            databases: crate::recent::load(core).await?.databases,
        },
    ))
}
