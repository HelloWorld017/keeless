use super::*;

pub(super) fn write_group(
    writer: &mut XmlWriter,
    group: &Group,
    db: &Database,
    inner_stream: &mut dyn InnerStreamCipher,
    use_binary_refs: bool,
    binary_index: &mut usize,
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
        )?;
        write_tag(writer, "IsExpanded", bool_xml(group.is_expanded))?;
        write_tag(
            writer,
            "DefaultAutoTypeSequence",
            &group.default_autotype_sequence,
        )?;
        super::data::write_custom_data(writer, &group.custom_data)?;

        for child_id in &group.child_group_ids {
            if let Some(child) = db.groups.get(child_id) {
                write_group(
                    writer,
                    child,
                    db,
                    inner_stream,
                    use_binary_refs,
                    binary_index,
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
                )?;
            }
        }
        Ok(())
    })
}
