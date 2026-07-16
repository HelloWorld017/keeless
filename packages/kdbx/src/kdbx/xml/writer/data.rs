use super::*;

pub(super) fn write_deleted_objects(
    writer: &mut XmlWriter,
    db: &Database,
    inner_stream: &mut dyn InnerStreamCipher,
) -> DatabaseResult<()> {
    if db.deleted_objects.is_empty() && db.xml_extensions.deleted_objects.is_empty() {
        return Ok(());
    }
    write_element(writer, "DeletedObjects", |writer| {
        for deleted in &db.deleted_objects {
            write_element(writer, "DeletedObject", |writer| {
                if let Some(uuid) = deleted.id.as_uuid() {
                    write_tag(writer, "UUID", &uuid_to_b64(uuid))?;
                }
                write_tag(
                    writer,
                    "DeletionTime",
                    &date_to_xml(&DateInstant::EpochMillis(deleted.deletion_time)),
                )?;
                if let Some(extensions) = db.xml_extensions.deleted_object.get(&deleted.id) {
                    write_preserved_elements(writer, extensions, inner_stream)?;
                }
                Ok(())
            })?;
        }
        write_preserved_elements(writer, &db.xml_extensions.deleted_objects, inner_stream)
    })
}

pub(super) fn write_custom_data(
    writer: &mut XmlWriter,
    data: &CustomData,
    inner_stream: &mut dyn InnerStreamCipher,
) -> DatabaseResult<()> {
    if data.is_empty() && data.xml_extensions.children.is_empty() {
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
                if let Some(extensions) = data.xml_extensions.items.get(key) {
                    write_preserved_elements(writer, extensions, inner_stream)?;
                }
                Ok(())
            })?;
        }
        write_preserved_elements(writer, &data.xml_extensions.children, inner_stream)
    })
}
