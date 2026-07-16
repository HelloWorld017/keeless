use super::super::helpers::*;

pub(super) fn read_meta<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    db: &mut Database,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => match tag(&e).as_str() {
                "DatabaseName" => db.name = read_text_content(reader, buf)?,
                "Generator" => {
                    let _ = read_text_content(reader, buf)?;
                }
                "DatabaseDescription" => db.description = read_text_content(reader, buf)?,
                "DefaultUserName" => db.default_username = read_text_content(reader, buf)?,
                "RecycleBinUUID" => {
                    let value = read_text_content(reader, buf)?;
                    db.recycle_bin_uuid = if value.is_empty() {
                        None
                    } else {
                        Some(required_uuid_from_b64(&value, "recycle bin")?)
                    };
                }
                "EntryTemplatesGroup" => {
                    let value = read_text_content(reader, buf)?;
                    db.entry_templates_uuid = if value.is_empty() {
                        None
                    } else {
                        Some(required_uuid_from_b64(&value, "entry templates group")?)
                    };
                }
                "CustomIcons" => read_custom_icons(reader, db, buf)?,
                "MemoryProtection" => read_memory_protection(reader, db, buf)?,
                "CustomData" => super::data::read_custom_data(
                    reader,
                    &mut db.custom_data,
                    &mut db.contains_unsupported_xml,
                    buf,
                )?,
                _ => {
                    db.contains_unsupported_xml = true;
                    skip_element(reader, e.name().as_ref())?;
                }
            },
            Event::End(e) if tag_end(&e) == "Meta" => return Ok(()),
            Event::Empty(e) => match tag(&e).as_str() {
                "Generator"
                | "DatabaseName"
                | "DatabaseDescription"
                | "DefaultUserName"
                | "RecycleBinUUID"
                | "EntryTemplatesGroup"
                | "CustomIcons"
                | "CustomData" => {}
                _ => db.contains_unsupported_xml = true,
            },
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

fn read_memory_protection<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    db: &mut Database,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => {
                let name = tag(&e);
                let value = read_text_content(reader, buf)? == "True";
                match name.as_str() {
                    "ProtectTitle" => db.memory_protection.protect_title = value,
                    "ProtectUserName" => db.memory_protection.protect_username = value,
                    "ProtectPassword" => db.memory_protection.protect_password = value,
                    "ProtectURL" => db.memory_protection.protect_url = value,
                    "ProtectNotes" => db.memory_protection.protect_notes = value,
                    "AutoEnableVisualHiding" => {
                        db.memory_protection.auto_enable_visual_hiding = value
                    }
                    _ => db.contains_unsupported_xml = true,
                }
            }
            Event::End(e) if tag_end(&e) == "MemoryProtection" => return Ok(()),
            Event::Empty(e) => match tag(&e).as_str() {
                "ProtectTitle"
                | "ProtectUserName"
                | "ProtectPassword"
                | "ProtectURL"
                | "ProtectNotes"
                | "AutoEnableVisualHiding" => {}
                _ => db.contains_unsupported_xml = true,
            },
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

fn read_custom_icons<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    db: &mut Database,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) if tag(&e) == "Icon" => {
                let mut uuid = Uuid::nil();
                let mut data = Vec::new();
                let mut name = String::new();
                loop {
                    buf.clear();
                    match reader.read_event_into(buf)? {
                        Event::Start(icon) => match tag(&icon).as_str() {
                            "UUID" => {
                                uuid = required_uuid_from_b64(
                                    &read_text_content(reader, buf)?,
                                    "custom icon",
                                )?
                            }
                            "Data" => {
                                let encoded = read_text_content(reader, buf)?;
                                data = base64::engine::general_purpose::STANDARD
                                    .decode(encoded.trim())
                                    .map_err(|err| {
                                        DatabaseError::InvalidFormat(format!(
                                            "invalid custom icon data: {err}"
                                        ))
                                    })?;
                            }
                            "Name" => name = read_text_content(reader, buf)?,
                            _ => {
                                db.contains_unsupported_xml = true;
                                skip_element(reader, icon.name().as_ref())?;
                            }
                        },
                        Event::End(icon) if tag_end(&icon) == "Icon" => break,
                        Event::Empty(icon) => match tag(&icon).as_str() {
                            "UUID" | "Data" | "Name" => {}
                            _ => db.contains_unsupported_xml = true,
                        },
                        Event::Eof => break,
                        _ => {}
                    }
                }
                db.custom_icons.insert(
                    uuid,
                    IconImageCustom {
                        uuid,
                        data,
                        name,
                        last_modification_time: 0,
                    },
                );
            }
            Event::End(e) if tag_end(&e) == "CustomIcons" => return Ok(()),
            Event::Empty(_) => db.contains_unsupported_xml = true,
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}
