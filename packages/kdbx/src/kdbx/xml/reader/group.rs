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
    let mut saw_uuid = false;
    let mut seen = std::collections::HashSet::new();

    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => {
                let name = tag(&e);
                if !matches!(name.as_str(), "Group" | "Entry")
                    && matches!(
                        name.as_str(),
                        "UUID"
                            | "Name"
                            | "Notes"
                            | "IconID"
                            | "CustomIconUUID"
                            | "IsExpanded"
                            | "EnableSearching"
                            | "DefaultAutoTypeSequence"
                            | "Times"
                            | "CustomData"
                    )
                    && !seen.insert(name.clone())
                {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "duplicate Group/{name} element"
                    )));
                }
                match name.as_str() {
                    "UUID" => {
                        let value = read_text_content(reader, buf)?;
                        group.id = NodeId::from_uuid(required_uuid_from_b64(&value, "group")?);
                        saw_uuid = true;
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
                        group.custom_icon_uuid =
                            Some(required_uuid_from_b64(&value, "group custom icon")?);
                    }
                    "IsExpanded" => {
                        group.is_expanded =
                            bool_from_xml(&read_text_content(reader, buf)?, "group IsExpanded")?
                    }
                    "EnableSearching" => {
                        group.enable_searching = match read_text_content(reader, buf)?.as_str() {
                            "True" | "true" => true,
                            "False" | "false" => false,
                            "Null" | "null" => true,
                            value => {
                                return Err(DatabaseError::InvalidFormat(format!(
                                    "invalid group EnableSearching value: {value}"
                                )))
                            }
                        };
                    }
                    "DefaultAutoTypeSequence" => {
                        group.default_autotype_sequence = read_text_content(reader, buf)?
                    }
                    "Times" => read_group_times(reader, &mut group, db, inner_stream, buf)?,
                    "Group" => {
                        let child = read_group(reader, db, inner_stream, binaries, buf)?;
                        if db.groups.contains_key(&child.id) {
                            return Err(DatabaseError::InvalidFormat(
                                "duplicate group UUID".into(),
                            ));
                        }
                        group.add_child_group(child.id);
                        db.groups.insert(child.id, child);
                    }
                    "Entry" => {
                        let entry =
                            super::entry::read_entry(reader, db, inner_stream, binaries, buf)?;
                        if db.entries.contains_key(&entry.id) {
                            return Err(DatabaseError::InvalidFormat(
                                "duplicate entry UUID".into(),
                            ));
                        }
                        group.add_child_entry(entry.id);
                        db.entries.insert(entry.id, entry);
                    }
                    "CustomData" => super::data::read_custom_data(
                        reader,
                        &mut group.custom_data,
                        &mut db.contains_unsupported_xml,
                        inner_stream,
                        buf,
                    )?,
                    _ => {
                        db.contains_unsupported_xml = true;
                        group.xml_extensions.children.push(preserve_element(
                            reader,
                            e,
                            inner_stream,
                        )?);
                    }
                }
            }
            Event::End(e) if tag_end(&e) == "Group" => break,
            Event::Empty(e) => {
                let name = tag(&e);
                if matches!(
                    name.as_str(),
                    "UUID"
                        | "Name"
                        | "Notes"
                        | "IconID"
                        | "CustomIconUUID"
                        | "IsExpanded"
                        | "EnableSearching"
                        | "DefaultAutoTypeSequence"
                        | "Times"
                        | "CustomData"
                ) && !seen.insert(name.clone())
                {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "duplicate Group/{name} element"
                    )));
                }
                match name.as_str() {
                    "UUID" => {
                        return Err(DatabaseError::InvalidFormat("group UUID is empty".into()))
                    }
                    "Name" => group.title.clear(),
                    "Notes" => group.notes.clear(),
                    "CustomIconUUID" => {
                        return Err(DatabaseError::InvalidFormat(
                            "group CustomIconUUID is empty".into(),
                        ))
                    }
                    "DefaultAutoTypeSequence" => group.default_autotype_sequence.clear(),
                    "Times" | "CustomData" => {}
                    "IconID" | "IsExpanded" | "EnableSearching" => {
                        return Err(DatabaseError::InvalidFormat(format!(
                            "Group/{name} value is empty"
                        )))
                    }
                    "Group" | "Entry" => {
                        return Err(DatabaseError::InvalidFormat(format!(
                            "empty {name} element is missing UUID"
                        )))
                    }
                    _ => {
                        db.contains_unsupported_xml = true;
                        group
                            .xml_extensions
                            .children
                            .push(preserve_empty_element(e)?);
                    }
                }
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of Group element".into(),
                ))
            }
            _ => {}
        }
    }

    if !saw_uuid {
        return Err(DatabaseError::InvalidFormat("group is missing UUID".into()));
    }
    if db.groups.contains_key(&group.id) {
        return Err(DatabaseError::InvalidFormat("duplicate group UUID".into()));
    }

    Ok(group)
}

fn read_group_times<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    group: &mut Group,
    db: &mut Database,
    inner_stream: &mut dyn InnerStreamCipher,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    let mut seen = std::collections::HashSet::new();
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => {
                let name = tag(&e);
                if matches!(
                    name.as_str(),
                    "CreationTime"
                        | "LastModificationTime"
                        | "LastAccessTime"
                        | "LocationChanged"
                        | "ExpiryTime"
                        | "Expires"
                        | "UsageCount"
                ) && !seen.insert(name.clone())
                {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "duplicate group Times/{name} element"
                    )));
                }
                match name.as_str() {
                    "CreationTime" => {
                        group.creation_time = date_from_xml(&read_text_content(reader, buf)?)?
                    }
                    "LastModificationTime" => {
                        group.last_modification_time =
                            date_from_xml(&read_text_content(reader, buf)?)?
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
                    "Expires" => {
                        group.expires =
                            bool_from_xml(&read_text_content(reader, buf)?, "group Expires")?
                    }
                    "UsageCount" => {
                        group.usage_count =
                            read_text_content(reader, buf)?.parse().map_err(|err| {
                                DatabaseError::InvalidFormat(format!(
                                    "invalid group usage count: {err}"
                                ))
                            })?
                    }
                    _ => {
                        db.contains_unsupported_xml = true;
                        group
                            .xml_extensions
                            .times
                            .push(preserve_element(reader, e, inner_stream)?);
                    }
                }
            }
            Event::End(e) if tag_end(&e) == "Times" => return Ok(()),
            Event::Empty(e) => {
                let name = tag(&e);
                if matches!(
                    name.as_str(),
                    "CreationTime"
                        | "LastModificationTime"
                        | "LastAccessTime"
                        | "LocationChanged"
                        | "ExpiryTime"
                        | "Expires"
                        | "UsageCount"
                ) && !seen.insert(name.clone())
                {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "duplicate group Times/{name} element"
                    )));
                }
                match name.as_str() {
                    "CreationTime"
                    | "LastModificationTime"
                    | "LastAccessTime"
                    | "LocationChanged"
                    | "ExpiryTime" => {}
                    "Expires" | "UsageCount" => {
                        return Err(DatabaseError::InvalidFormat(format!(
                            "group Times/{name} value is empty"
                        )))
                    }
                    _ => {
                        db.contains_unsupported_xml = true;
                        group.xml_extensions.times.push(preserve_empty_element(e)?);
                    }
                }
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of group Times element".into(),
                ))
            }
            _ => {}
        }
    }
}
