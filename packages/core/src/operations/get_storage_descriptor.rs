use keeless_schema::{GetStorageDescriptorArgs, OperationSuccess, StorageDescriptorResult};

use crate::{KeelessCore, Result, StorageDescriptor};

pub(crate) fn run(core: &mut KeelessCore) -> Option<StorageDescriptor> {
    core.enforce_auto_lock();
    let descriptor = core
        .selection
        .as_ref()
        .map(|selection| selection.descriptor.clone());
    core.touch_activity();
    descriptor
}

pub(super) fn execute(
    core: &mut KeelessCore,
    _args: GetStorageDescriptorArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::GetStorageDescriptor(
        StorageDescriptorResult { storage: run(core) },
    ))
}
