use super::*;

pub(super) fn write_group(
    writer: &mut XmlWriter,
    group: &Group,
    db: &Database,
    inner_stream: &mut dyn InnerStreamCipher,
    use_binary_refs: bool,
    binary_index: &mut usize,
    memory: &mut MemoryWriteAccess<'_>,
) -> DatabaseResult<()> {
    write_element(writer, "Group", |writer| {
        if let Some(uuid) = group.id.as_uuid() {
            write_tag(writer, "UUID", &uuid_to_b64(uuid))?;
        }
        write_tag(writer, "Name", &group.title)?;
        if !group.notes.is_empty() {
            write_tag(writer, "Notes", &group.notes)?;
        }
        write_icon_id(writer, &group.icon)?;
        if let Some(uuid) = group.custom_icon_uuid {
            write_tag(writer, "CustomIconUUID", &uuid_to_b64(&uuid))?;
        }
        write_times(
            writer,
            group.creation_time,
            group.last_modification_time,
            group.last_access_time,
            group.expiry_time,
            group.expires,
            group.usage_count,
            group.location_changed,
            &group.xml_extensions.times,
            inner_stream,
        )?;
        write_tag(writer, "IsExpanded", bool_xml(group.is_expanded))?;
        write_tag(
            writer,
            "DefaultAutoTypeSequence",
            &group.default_autotype_sequence,
        )?;
        write_tag(writer, "EnableSearching", bool_xml(group.enable_searching))?;
        super::data::write_custom_data(writer, &group.custom_data, inner_stream)?;

        for child_id in &group.child_group_ids {
            if let Some(child) = db.groups.get(child_id) {
                write_group(
                    writer,
                    child,
                    db,
                    inner_stream,
                    use_binary_refs,
                    binary_index,
                    memory,
                )?;
            }
        }
        for entry_id in &group.child_entry_ids {
            if let Some(entry) = db.entries.get(entry_id) {
                super::entry::write_entry(
                    writer,
                    entry,
                    inner_stream,
                    use_binary_refs,
                    binary_index,
                    memory,
                )?;
            }
        }
        write_preserved_elements(writer, &group.xml_extensions.children, inner_stream)?;
        Ok(())
    })
}
