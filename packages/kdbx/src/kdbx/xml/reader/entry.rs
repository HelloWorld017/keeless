mod auto_type;
mod binary;
mod field;
mod history;
mod times;

use super::super::helpers::*;

pub(super) fn read_entry<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    db: &mut Database,
    inner_stream: &mut dyn InnerStreamCipher,
    binaries: &[(Vec<u8>, bool)],
    buf: &mut Vec<u8>,
) -> DatabaseResult<Entry> {
    let mut entry = Entry::new(NodeId::new_uuid());
    let mut saw_uuid = false;
    let mut seen = std::collections::HashSet::new();

    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(event) => {
                let name = tag(&event);
                if matches!(
                    name.as_str(),
                    "UUID"
                        | "IconID"
                        | "CustomIconUUID"
                        | "ForegroundColor"
                        | "BackgroundColor"
                        | "OverrideURL"
                        | "Tags"
                        | "AutoType"
                        | "Times"
                        | "History"
                        | "CustomData"
                ) && !seen.insert(name.clone())
                {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "duplicate Entry/{name} element"
                    )));
                }
                match name.as_str() {
                    "UUID" => {
                        let value = read_text_content(reader, buf)?;
                        entry.id = NodeId::from_uuid(required_uuid_from_b64(&value, "entry")?);
                        saw_uuid = true;
                    }
                    "IconID" => {
                        let id = read_text_content(reader, buf)?;
                        entry.icon =
                            IconImage::Standard(IconImageStandard::new(icon_id_from_str(&id)?));
                    }
                    "CustomIconUUID" => {
                        let value = read_text_content(reader, buf)?;
                        entry.custom_icon_uuid =
                            Some(required_uuid_from_b64(&value, "entry custom icon")?);
                    }
                    "ForegroundColor" => entry.foreground_color = read_text_content(reader, buf)?,
                    "BackgroundColor" => entry.background_color = read_text_content(reader, buf)?,
                    "OverrideURL" => entry.override_url = read_text_content(reader, buf)?,
                    "Tags" => {
                        let value = read_text_content(reader, buf)?;
                        entry.tags = if value.is_empty() {
                            Vec::new()
                        } else {
                            value
                                .split(';')
                                .map(|tag| tag.trim().to_string())
                                .filter(|tag| !tag.is_empty())
                                .collect()
                        };
                    }
                    "String" => field::read_entry_string(
                        reader,
                        &mut entry,
                        inner_stream,
                        &mut db.contains_unsupported_xml,
                        buf,
                    )?,
                    "Binary" => binary::read_entry_binary(
                        reader,
                        &mut entry,
                        binaries,
                        inner_stream,
                        &mut db.contains_unsupported_xml,
                        buf,
                    )?,
                    "AutoType" => auto_type::read_auto_type(
                        reader,
                        &mut entry,
                        inner_stream,
                        &mut db.contains_unsupported_xml,
                        buf,
                    )?,
                    "Times" => times::read_entry_times(reader, &mut entry, db, inner_stream, buf)?,
                    "History" => {
                        history::read_history(reader, &mut entry, db, inner_stream, binaries, buf)?
                    }
                    "CustomData" => super::data::read_custom_data(
                        reader,
                        &mut entry.custom_data,
                        &mut db.contains_unsupported_xml,
                        inner_stream,
                        buf,
                    )?,
                    _ => {
                        db.contains_unsupported_xml = true;
                        entry.xml_extensions.children.push(preserve_element(
                            reader,
                            event,
                            inner_stream,
                        )?);
                    }
                }
            }
            Event::End(event) if tag_end(&event) == "Entry" => break,
            Event::Empty(event) => {
                let name = tag(&event);
                if matches!(
                    name.as_str(),
                    "UUID"
                        | "IconID"
                        | "CustomIconUUID"
                        | "ForegroundColor"
                        | "BackgroundColor"
                        | "OverrideURL"
                        | "Tags"
                        | "AutoType"
                        | "Times"
                        | "History"
                        | "CustomData"
                ) && !seen.insert(name.clone())
                {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "duplicate Entry/{name} element"
                    )));
                }
                match name.as_str() {
                    "UUID" => {
                        return Err(DatabaseError::InvalidFormat("entry UUID is empty".into()))
                    }
                    "CustomIconUUID" => {
                        return Err(DatabaseError::InvalidFormat(
                            "entry CustomIconUUID is empty".into(),
                        ))
                    }
                    "ForegroundColor" => entry.foreground_color.clear(),
                    "BackgroundColor" => entry.background_color.clear(),
                    "OverrideURL" => entry.override_url.clear(),
                    "Tags" => entry.tags.clear(),
                    "AutoType" | "Times" | "History" | "CustomData" => {}
                    "IconID" => {
                        return Err(DatabaseError::InvalidFormat("entry IconID is empty".into()))
                    }
                    "String" | "Binary" => {
                        return Err(DatabaseError::InvalidFormat(format!(
                            "empty {name} element is missing Key and Value"
                        )))
                    }
                    _ => {
                        db.contains_unsupported_xml = true;
                        entry
                            .xml_extensions
                            .children
                            .push(preserve_empty_element(event)?);
                    }
                }
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of Entry element".into(),
                ))
            }
            _ => {}
        }
    }

    if !saw_uuid {
        return Err(DatabaseError::InvalidFormat("entry is missing UUID".into()));
    }

    Ok(entry)
}
