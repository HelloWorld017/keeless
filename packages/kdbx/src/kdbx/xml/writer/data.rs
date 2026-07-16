use super::*;

pub(super) fn write_deleted_objects(
    writer: &mut XmlWriter,
    deleted_objects: &[DeletedObject],
) -> DatabaseResult<()> {
    if deleted_objects.is_empty() {
        return Ok(());
    }
    write_element(writer, "DeletedObjects", |writer| {
        for deleted in deleted_objects {
            write_element(writer, "DeletedObject", |writer| {
                if let Some(uuid) = deleted.id.as_uuid() {
                    write_tag(writer, "UUID", &uuid_to_b64(uuid))?;
                }
                write_tag(
                    writer,
                    "DeletionTime",
                    &date_to_xml(&DateInstant::EpochMillis(deleted.deletion_time)),
                )
            })?;
        }
        Ok(())
    })
}

pub(super) fn write_custom_data(writer: &mut XmlWriter, data: &CustomData) -> DatabaseResult<()> {
    if data.is_empty() {
        return Ok(());
    }
    write_element(writer, "CustomData", |writer| {
        for (key, item) in data.iter() {
            write_element(writer, "Item", |writer| {
                write_tag(writer, "Key", key)?;
                write_tag(writer, "Value", &item.value)?;
                if let Some(time) = item.last_modification_time {
                    write_tag(
                        writer,
                        "LastModificationTime",
                        &date_to_xml(&DateInstant::EpochMillis(time)),
                    )?;
                }
                Ok(())
            })?;
        }
        Ok(())
    })
}
