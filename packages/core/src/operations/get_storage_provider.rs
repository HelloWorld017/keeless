use keeless_schema::{GetStorageProviderArgs, OperationSuccess, StorageProviderResult};

use crate::{KeelessCore, Result};

pub(crate) fn run(core: &mut KeelessCore) -> Option<String> {
    let provider = core
        .selection
        .as_ref()
        .and_then(|selection| selection.descriptor.as_ref())
        .map(|descriptor| descriptor.provider.clone());
    core.touch_activity();
    provider
}

pub(super) fn execute(
    core: &mut KeelessCore,
    _args: GetStorageProviderArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetStorageProvider(
        StorageProviderResult {
            provider: run(core),
        },
    ))
}
