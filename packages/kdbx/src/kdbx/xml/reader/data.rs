use super::super::helpers::*;

pub(super) fn read_deleted_objects<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    db: &mut Database,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) if tag(&e) == "DeletedObject" => {
                let mut id = None;
                let mut deletion_time = 0;
                loop {
                    buf.clear();
                    match reader.read_event_into(buf)? {
                        Event::Start(item) => match tag(&item).as_str() {
                            "UUID" => {
                                let value = read_text_content(reader, buf)?;
                                id = Some(NodeId::from_uuid(required_uuid_from_b64(
                                    &value,
                                    "deleted object",
                                )?));
                            }
                            "DeletionTime" => {
                                deletion_time = date_from_xml(&read_text_content(reader, buf)?)?
                                    .as_millis()
                                    .ok_or_else(|| {
                                        DatabaseError::InvalidFormat(
                                            "deleted object is missing deletion time".into(),
                                        )
                                    })?
                            }
                            _ => {
                                db.contains_unsupported_xml = true;
                                skip_element(reader, item.name().as_ref())?;
                            }
                        },
                        Event::End(item) if tag_end(&item) == "DeletedObject" => break,
                        Event::Empty(_) => db.contains_unsupported_xml = true,
                        Event::Eof => break,
                        _ => {}
                    }
                }
                db.deleted_objects.push(DeletedObject {
                    id: id.ok_or_else(|| {
                        DatabaseError::InvalidFormat("deleted object is missing UUID".into())
                    })?,
                    deletion_time,
                });
            }
            Event::End(e) if tag_end(&e) == "DeletedObjects" => return Ok(()),
            Event::Start(e) => {
                db.contains_unsupported_xml = true;
                skip_element(reader, e.name().as_ref())?;
            }
            Event::Empty(_) => db.contains_unsupported_xml = true,
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

pub(super) fn read_custom_data<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    data: &mut CustomData,
    unsupported: &mut bool,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) if tag(&e) == "Item" => {
                let mut key = String::new();
                let mut value = String::new();
                let mut last_modification_time = None;
                loop {
                    buf.clear();
                    match reader.read_event_into(buf)? {
                        Event::Start(item) => match tag(&item).as_str() {
                            "Key" => key = read_text_content(reader, buf)?,
                            "Value" => value = read_text_content(reader, buf)?,
                            "LastModificationTime" => {
                                last_modification_time =
                                    date_from_xml(&read_text_content(reader, buf)?)?.as_millis()
                            }
                            _ => {
                                *unsupported = true;
                                skip_element(reader, item.name().as_ref())?;
                            }
                        },
                        Event::End(item) if tag_end(&item) == "Item" => break,
                        Event::Empty(item) => match tag(&item).as_str() {
                            "Key" | "Value" | "LastModificationTime" => {}
                            _ => *unsupported = true,
                        },
                        Event::Eof => break,
                        _ => {}
                    }
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
                skip_element(reader, e.name().as_ref())?;
            }
            Event::Empty(_) => *unsupported = true,
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}
