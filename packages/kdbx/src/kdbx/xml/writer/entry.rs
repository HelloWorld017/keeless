use super::*;

pub(super) fn write_entry(
    writer: &mut XmlWriter,
    entry: &Entry,
    inner_stream: &mut dyn InnerStreamCipher,
    use_binary_refs: bool,
    binary_index: &mut usize,
    memory: &mut MemoryWriteAccess<'_>,
) -> DatabaseResult<()> {
    write_element(writer, "Entry", |writer| {
        if let Some(uuid) = entry.id.as_uuid() {
            write_tag(writer, "UUID", &uuid_to_b64(uuid))?;
        }
        write_icon_id(writer, &entry.icon)?;
        if let Some(uuid) = entry.custom_icon_uuid {
            write_tag(writer, "CustomIconUUID", &uuid_to_b64(&uuid))?;
        }
        if !entry.foreground_color.is_empty() {
            write_tag(writer, "ForegroundColor", &entry.foreground_color)?;
        }
        if !entry.background_color.is_empty() {
            write_tag(writer, "BackgroundColor", &entry.background_color)?;
        }
        if !entry.override_url.is_empty() {
            write_tag(writer, "OverrideURL", &entry.override_url)?;
        }
        if !entry.tags.is_empty() {
            write_tag(writer, "Tags", &entry.tags.join(";"))?;
        }

        write_fields(writer, entry, inner_stream, memory)?;
        write_auto_type(writer, entry, inner_stream)?;
        write_binaries(writer, entry, use_binary_refs, binary_index, inner_stream)?;
        super::data::write_custom_data(writer, &entry.custom_data, inner_stream)?;
        write_times(
            writer,
            entry.creation_time,
            entry.last_modification_time,
            entry.last_access_time,
            entry.expiry_time,
            entry.expires,
            entry.usage_count,
            entry.location_changed,
            &entry.xml_extensions.times,
            inner_stream,
        )?;
        if !entry.history.is_empty() || !entry.xml_extensions.history.is_empty() {
            write_element(writer, "History", |writer| {
                for history_entry in &entry.history {
                    write_entry(
                        writer,
                        history_entry,
                        inner_stream,
                        use_binary_refs,
                        binary_index,
                        memory,
                    )?;
                }
                write_preserved_elements(writer, &entry.xml_extensions.history, inner_stream)
            })?;
        }
        write_preserved_elements(writer, &entry.xml_extensions.children, inner_stream)?;
        Ok(())
    })
}

fn write_fields(
    writer: &mut XmlWriter,
    entry: &Entry,
    inner_stream: &mut dyn InnerStreamCipher,
    memory: &mut MemoryWriteAccess<'_>,
) -> DatabaseResult<()> {
    write_memory_field(
        writer,
        entry,
        "Title",
        &MemoryField::Title,
        entry.title_is_protected,
        inner_stream,
        memory,
    )?;
    write_memory_field(
        writer,
        entry,
        "UserName",
        &MemoryField::UserName,
        entry.username.is_protected(),
        inner_stream,
        memory,
    )?;
    write_memory_field(
        writer,
        entry,
        "Password",
        &MemoryField::Password,
        entry.password.is_protected(),
        inner_stream,
        memory,
    )?;
    write_memory_field(
        writer,
        entry,
        "URL",
        &MemoryField::Url,
        entry.url_is_protected,
        inner_stream,
        memory,
    )?;
    write_memory_field(
        writer,
        entry,
        "Notes",
        &MemoryField::Notes,
        entry.notes.is_protected(),
        inner_stream,
        memory,
    )?;
    for field in &entry.custom_fields {
        write_memory_field(
            writer,
            entry,
            &field.name,
            &MemoryField::Custom(field.name.clone()),
            field.is_protected,
            inner_stream,
            memory,
        )?;
    }
    Ok(())
}

fn write_memory_field(
    writer: &mut XmlWriter,
    entry: &Entry,
    key: &str,
    field: &MemoryField,
    protect: bool,
    inner_stream: &mut dyn InnerStreamCipher,
    memory: &mut MemoryWriteAccess<'_>,
) -> DatabaseResult<()> {
    memory.with_field(entry, field, |value| {
        write_field(
            writer,
            key,
            value,
            protect,
            inner_stream,
            entry
                .xml_extensions
                .strings
                .get(key)
                .map(Vec::as_slice)
                .unwrap_or_default(),
        )
    })
}

fn write_field(
    writer: &mut XmlWriter,
    key: &str,
    value: &str,
    protect: bool,
    inner_stream: &mut dyn InnerStreamCipher,
    extensions: &[PreservedXmlElement],
) -> DatabaseResult<()> {
    write_element(writer, "String", |writer| {
        write_tag(writer, "Key", key)?;
        if protect {
            let mut bytes = value.as_bytes().to_vec();
            inner_stream.process(&mut bytes)?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let mut value_element = BytesStart::new("Value");
            value_element.push_attribute(("Protected", "True"));
            writer
                .write_event(Event::Start(value_element))
                .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?;
            if !encoded.is_empty() {
                writer
                    .write_event(Event::Text(BytesText::new(&encoded)))
                    .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?;
            }
            writer
                .write_event(Event::End(BytesEnd::new("Value")))
                .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?;
        } else {
            write_tag(writer, "Value", value)?;
        }
        write_preserved_elements(writer, extensions, inner_stream)?;
        Ok(())
    })
}

fn write_auto_type(
    writer: &mut XmlWriter,
    entry: &Entry,
    inner_stream: &mut dyn InnerStreamCipher,
) -> DatabaseResult<()> {
    write_element(writer, "AutoType", |writer| {
        write_tag(writer, "Enabled", bool_xml(entry.auto_type.enabled))?;
        write_tag(writer, "DefaultSequence", &entry.auto_type.default_sequence)?;
        for (index, association) in entry.auto_type.associations.iter().enumerate() {
            write_element(writer, "Association", |writer| {
                write_tag(writer, "Window", &association.window_title)?;
                write_tag(writer, "KeystrokeSequence", &association.keystroke_sequence)?;
                if let Some(extensions) = entry.xml_extensions.associations.get(index) {
                    write_preserved_elements(writer, extensions, inner_stream)?;
                }
                Ok(())
            })?;
        }
        write_preserved_elements(writer, &entry.xml_extensions.auto_type, inner_stream)
    })
}

fn write_binaries(
    writer: &mut XmlWriter,
    entry: &Entry,
    use_binary_refs: bool,
    binary_index: &mut usize,
    inner_stream: &mut dyn InnerStreamCipher,
) -> DatabaseResult<()> {
    for binary in &entry.binaries {
        write_element(writer, "Binary", |writer| {
            write_tag(writer, "Key", &binary.name)?;
            if use_binary_refs {
                let mut value = BytesStart::new("Value");
                let reference = binary_index.to_string();
                value.push_attribute(("Ref", reference.as_str()));
                writer
                    .write_event(Event::Empty(value))
                    .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?;
                *binary_index += 1;
            } else {
                write_tag(
                    writer,
                    "Value",
                    &base64::engine::general_purpose::STANDARD.encode(&binary.data),
                )?;
            }
            if let Some(extensions) = entry.xml_extensions.binaries.get(&binary.name) {
                write_preserved_elements(writer, extensions, inner_stream)?;
            }
            Ok(())
        })?;
    }
    Ok(())
}
