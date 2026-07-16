use super::super::helpers::*;

pub(super) fn read_deleted_objects<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    db: &mut Database,
    inner_stream: &mut dyn InnerStreamCipher,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) if tag(&e) == "DeletedObject" => {
                let mut id = None;
                let mut deletion_time = None;
                let mut extensions = Vec::new();
                let mut seen = std::collections::HashSet::new();
                loop {
                    buf.clear();
                    match reader.read_event_into(buf)? {
                        Event::Start(item) => {
                            let name = tag(&item);
                            if matches!(name.as_str(), "UUID" | "DeletionTime")
                                && !seen.insert(name.clone())
                            {
                                return Err(DatabaseError::InvalidFormat(format!(
                                    "duplicate DeletedObject/{name} element"
                                )));
                            }
                            match name.as_str() {
                                "UUID" => {
                                    let value = read_text_content(reader, buf)?;
                                    id = Some(NodeId::from_uuid(required_uuid_from_b64(
                                        &value,
                                        "deleted object",
                                    )?));
                                }
                                "DeletionTime" => {
                                    deletion_time = Some(
                                        date_from_xml(&read_text_content(reader, buf)?)?
                                            .as_millis()
                                            .ok_or_else(|| {
                                                DatabaseError::InvalidFormat(
                                                    "deleted object is missing deletion time"
                                                        .into(),
                                                )
                                            })?,
                                    )
                                }
                                _ => {
                                    db.contains_unsupported_xml = true;
                                    extensions.push(preserve_element(reader, item, inner_stream)?);
                                }
                            }
                        }
                        Event::End(item) if tag_end(&item) == "DeletedObject" => break,
                        Event::Empty(item) => match tag(&item).as_str() {
                            "UUID" | "DeletionTime" => {
                                return Err(DatabaseError::InvalidFormat(format!(
                                    "DeletedObject/{} value is empty",
                                    tag(&item)
                                )))
                            }
                            _ => {
                                db.contains_unsupported_xml = true;
                                extensions.push(preserve_empty_element(item)?);
                            }
                        },
                        Event::Eof => {
                            return Err(DatabaseError::InvalidFormat(
                                "unexpected end of DeletedObject element".into(),
                            ))
                        }
                        _ => {}
                    }
                }
                let id = id.ok_or_else(|| {
                    DatabaseError::InvalidFormat("deleted object is missing UUID".into())
                })?;
                if db.deleted_objects.iter().any(|item| item.id == id) {
                    return Err(DatabaseError::InvalidFormat(
                        "duplicate deleted object UUID".into(),
                    ));
                }
                let deletion_time = deletion_time.ok_or_else(|| {
                    DatabaseError::InvalidFormat("deleted object is missing deletion time".into())
                })?;
                if !extensions.is_empty() {
                    db.xml_extensions.deleted_object.insert(id, extensions);
                }
                db.deleted_objects.push(DeletedObject { id, deletion_time });
            }
            Event::End(e) if tag_end(&e) == "DeletedObjects" => return Ok(()),
            Event::Start(e) => {
                db.contains_unsupported_xml = true;
                db.xml_extensions
                    .deleted_objects
                    .push(preserve_element(reader, e, inner_stream)?);
            }
            Event::Empty(e) if tag(&e) == "DeletedObject" => {
                return Err(DatabaseError::InvalidFormat(
                    "empty DeletedObject element".into(),
                ))
            }
            Event::Empty(e) => {
                db.contains_unsupported_xml = true;
                db.xml_extensions
                    .deleted_objects
                    .push(preserve_empty_element(e)?);
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of DeletedObjects element".into(),
                ))
            }
            _ => {}
        }
    }
}

pub(super) fn read_custom_data<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    data: &mut CustomData,
    unsupported: &mut bool,
    inner_stream: &mut dyn InnerStreamCipher,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) if tag(&e) == "Item" => {
                let mut key = None;
                let mut value = None;
                let mut last_modification_time = None;
                let mut extensions = Vec::new();
                let mut seen = std::collections::HashSet::new();
                loop {
                    buf.clear();
                    match reader.read_event_into(buf)? {
                        Event::Start(item) => {
                            let name = tag(&item);
                            if matches!(name.as_str(), "Key" | "Value" | "LastModificationTime")
                                && !seen.insert(name.clone())
                            {
                                return Err(DatabaseError::InvalidFormat(format!(
                                    "duplicate CustomData/Item/{name} element"
                                )));
                            }
                            match name.as_str() {
                                "Key" => key = Some(read_text_content(reader, buf)?),
                                "Value" => value = Some(read_text_content(reader, buf)?),
                                "LastModificationTime" => {
                                    last_modification_time =
                                        date_from_xml(&read_text_content(reader, buf)?)?.as_millis()
                                }
                                _ => {
                                    *unsupported = true;
                                    extensions.push(preserve_element(reader, item, inner_stream)?);
                                }
                            }
                        }
                        Event::End(item) if tag_end(&item) == "Item" => break,
                        Event::Empty(item) => {
                            let name = tag(&item);
                            if matches!(name.as_str(), "Key" | "Value" | "LastModificationTime")
                                && !seen.insert(name.clone())
                            {
                                return Err(DatabaseError::InvalidFormat(format!(
                                    "duplicate CustomData/Item/{name} element"
                                )));
                            }
                            match name.as_str() {
                                "Key" => key = Some(String::new()),
                                "Value" => value = Some(String::new()),
                                "LastModificationTime" => {
                                    return Err(DatabaseError::InvalidFormat(
                                        "CustomData modification time is empty".into(),
                                    ))
                                }
                                _ => {
                                    *unsupported = true;
                                    extensions.push(preserve_empty_element(item)?);
                                }
                            }
                        }
                        Event::Eof => {
                            return Err(DatabaseError::InvalidFormat(
                                "unexpected end of CustomData Item".into(),
                            ))
                        }
                        _ => {}
                    }
                }
                let key = key.ok_or_else(|| {
                    DatabaseError::InvalidFormat("CustomData Item is missing Key".into())
                })?;
                let value = value.ok_or_else(|| {
                    DatabaseError::InvalidFormat("CustomData Item is missing Value".into())
                })?;
                if data.get(&key).is_some() {
                    return Err(DatabaseError::InvalidFormat(
                        "duplicate CustomData key".into(),
                    ));
                }
                if !extensions.is_empty() {
                    data.xml_extensions.items.insert(key.clone(), extensions);
                }
                data.insert(
                    key,
                    CustomDataItem {
                        value,
                        last_modification_time,
                    },
                );
            }
            Event::End(e) if tag_end(&e) == "CustomData" => return Ok(()),
            Event::Start(e) => {
                *unsupported = true;
                data.xml_extensions
                    .children
                    .push(preserve_element(reader, e, inner_stream)?);
            }
            Event::Empty(e) if tag(&e) == "Item" => {
                return Err(DatabaseError::InvalidFormat("empty CustomData Item".into()))
            }
            Event::Empty(e) => {
                *unsupported = true;
                data.xml_extensions
                    .children
                    .push(preserve_empty_element(e)?);
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of CustomData element".into(),
                ))
            }
            _ => {}
        }
    }
}
