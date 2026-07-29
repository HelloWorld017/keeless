use keeless_schema::{
    OperationSuccess, PrepareEntryAttachmentDownloadArgs, PrepareEntryAttachmentDownloadResult,
};
use zeroize::Zeroizing;

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) fn execute(
    core: &mut KeelessCore,
    args: PrepareEntryAttachmentDownloadArgs,
) -> Result<OperationSuccess> {
    let entry_id = parse_node_id(args.entry_id)?;
    let attachment_index =
        usize::try_from(args.attachment_index).map_err(|_| CoreError::AttachmentNotFound)?;
    let attachment = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database()
        .get_entry(&entry_id)
        .ok_or(CoreError::EntryNotFound)?
        .binaries
        .get(attachment_index)
        .filter(|attachment| attachment.name == args.name)
        .ok_or(CoreError::AttachmentNotFound)?;
    let transfer_id = core.publish_download_transfer(Zeroizing::new(attachment.data.clone()))?;
    Ok(OperationSuccess::PrepareEntryAttachmentDownload(
        PrepareEntryAttachmentDownloadResult { transfer_id },
    ))
}
