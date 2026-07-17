use keeless_schema::{EmptyResult, OpenArgs, OperationSuccess};

use crate::{CoreError, KeelessCore, Result, Selection, StorageDescriptor};

pub(crate) async fn run(core: &mut KeelessCore, descriptor: StorageDescriptor) -> Result<()> {
    let provider = core
        .storage_providers
        .get(&descriptor.provider)
        .cloned()
        .ok_or_else(|| CoreError::UnknownStorageProvider(descriptor.provider.clone()))?;
    let exists = provider.stat(&descriptor.path).await?.is_some();
    super::lock::run(core);
    core.selection = Some(Selection {
        descriptor,
        provider,
        exists,
    });
    Ok(())
}

pub(super) async fn execute(core: &mut KeelessCore, args: OpenArgs) -> Result<OperationSuccess> {
    run(core, args.storage).await?;
    Ok(OperationSuccess::Open(EmptyResult {}))
}
