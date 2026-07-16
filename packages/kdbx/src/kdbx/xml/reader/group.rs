use super::super::helpers::*;

/// Recursively read a `<Group>` element and insert its descendants into `db`.
pub(super) fn read_group<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    db: &mut Database,
    inner_stream: &mut dyn InnerStreamCipher,
    binaries: &[(Vec<u8>, bool)],
    buf: &mut Vec<u8>,
) -> DatabaseResult<Group> {
    let mut group = Group::new(NodeId::new_uuid());

    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => match tag(&e).as_str() {
                "UUID" => {
                    let value = read_text_content(reader, buf)?;
                    group.id = NodeId::from_uuid(required_uuid_from_b64(&value, "group")?);
                }
                "Name" => group.title = read_text_content(reader, buf)?,
                "Notes" => group.notes = read_text_content(reader, buf)?,
                "IconID" => {
                    let id = read_text_content(reader, buf)?;
                    group.icon =
                        IconImage::Standard(IconImageStandard::new(icon_id_from_str(&id)?));
                }
                "CustomIconUUID" => {
                    let value = read_text_content(reader, buf)?;
                    group.custom_icon_uuid = if value.is_empty() {
                        None
                    } else {
                        Some(required_uuid_from_b64(&value, "group custom icon")?)
                    };
                }
                "IsExpanded" => group.is_expanded = read_text_content(reader, buf)? != "False",
                "EnableSearching" => {
                    group.enable_searching = read_text_content(reader, buf)? != "False";
                }
                "DefaultAutoTypeSequence" => {
                    group.default_autotype_sequence = read_text_content(reader, buf)?
                }
                "Times" => read_group_times(reader, &mut group, db, buf)?,
                "Group" => {
                    let child = read_group(reader, db, inner_stream, binaries, buf)?;
                    group.add_child_group(child.id);
                    db.groups.insert(child.id, child);
                }
                "Entry" => {
                    let entry = super::entry::read_entry(reader, db, inner_stream, binaries, buf)?;
                    group.add_child_entry(entry.id);
                    db.entries.insert(entry.id, entry);
                }
                "CustomData" => super::data::read_custom_data(
                    reader,
                    &mut group.custom_data,
                    &mut db.contains_unsupported_xml,
                    buf,
                )?,
                _ => {
                    db.contains_unsupported_xml = true;
                    skip_element(reader, e.name().as_ref())?;
                }
            },
            Event::End(e) if tag_end(&e) == "Group" => break,
            Event::Empty(e) => match tag(&e).as_str() {
                "UUID"
                | "Name"
                | "Notes"
                | "CustomIconUUID"
                | "DefaultAutoTypeSequence"
                | "CustomData" => {}
                _ => db.contains_unsupported_xml = true,
            },
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(group)
}

fn read_group_times<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    group: &mut Group,
    db: &mut Database,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => match tag(&e).as_str() {
                "CreationTime" => {
                    group.creation_time = date_from_xml(&read_text_content(reader, buf)?)?
                }
                "LastModificationTime" => {
                    group.last_modification_time = date_from_xml(&read_text_content(reader, buf)?)?
                }
                "LastAccessTime" => {
                    group.last_access_time = date_from_xml(&read_text_content(reader, buf)?)?
                }
                "LocationChanged" => {
                    group.location_changed = date_from_xml(&read_text_content(reader, buf)?)?
                }
                "ExpiryTime" => {
                    group.expiry_time = date_from_xml(&read_text_content(reader, buf)?)?
                }
                "Expires" => group.expires = read_text_content(reader, buf)? == "True",
                "UsageCount" => {
                    group.usage_count = read_text_content(reader, buf)?.parse().map_err(|err| {
                        DatabaseError::InvalidFormat(format!("invalid group usage count: {err}"))
                    })?
                }
                _ => {
                    db.contains_unsupported_xml = true;
                    skip_element(reader, e.name().as_ref())?;
                }
            },
            Event::End(e) if tag_end(&e) == "Times" => return Ok(()),
            Event::Empty(e) => match tag(&e).as_str() {
                "CreationTime"
                | "LastModificationTime"
                | "LastAccessTime"
                | "LocationChanged"
                | "ExpiryTime" => {}
                _ => db.contains_unsupported_xml = true,
            },
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}
