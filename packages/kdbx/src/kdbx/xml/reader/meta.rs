use super::super::helpers::*;

pub(super) fn read_meta<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
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
                    "DatabaseName"
                        | "Generator"
                        | "DatabaseDescription"
                        | "DefaultUserName"
                        | "RecycleBinUUID"
                        | "EntryTemplatesGroup"
                        | "CustomIcons"
                        | "MemoryProtection"
                        | "CustomData"
                ) && !seen.insert(name.clone())
                {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "duplicate Meta/{name} element"
                    )));
                }
                match name.as_str() {
                    "DatabaseName" => db.name = read_text_content(reader, buf)?,
                    "Generator" => {
                        let _ = read_text_content(reader, buf)?;
                    }
                    "DatabaseDescription" => db.description = read_text_content(reader, buf)?,
                    "DefaultUserName" => db.default_username = read_text_content(reader, buf)?,
                    "RecycleBinUUID" => {
                        let value = read_text_content(reader, buf)?;
                        db.recycle_bin_uuid = Some(required_uuid_from_b64(&value, "recycle bin")?);
                    }
                    "EntryTemplatesGroup" => {
                        let value = read_text_content(reader, buf)?;
                        db.entry_templates_uuid =
                            Some(required_uuid_from_b64(&value, "entry templates group")?);
                    }
                    "CustomIcons" => read_custom_icons(reader, db, inner_stream, buf)?,
                    "MemoryProtection" => read_memory_protection(reader, db, inner_stream, buf)?,
                    "CustomData" => super::data::read_custom_data(
                        reader,
                        &mut db.custom_data,
                        &mut db.contains_unsupported_xml,
                        inner_stream,
                        buf,
                    )?,
                    _ => {
                        db.contains_unsupported_xml = true;
                        db.xml_extensions
                            .meta
                            .push(preserve_element(reader, e, inner_stream)?);
                    }
                }
            }
            Event::End(e) if tag_end(&e) == "Meta" => return Ok(()),
            Event::Empty(e) => {
                let name = tag(&e);
                if matches!(
                    name.as_str(),
                    "DatabaseName"
                        | "Generator"
                        | "DatabaseDescription"
                        | "DefaultUserName"
                        | "RecycleBinUUID"
                        | "EntryTemplatesGroup"
                        | "CustomIcons"
                        | "MemoryProtection"
                        | "CustomData"
                ) && !seen.insert(name.clone())
                {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "duplicate Meta/{name} element"
                    )));
                }
                match name.as_str() {
                    "Generator"
                    | "DatabaseName"
                    | "DatabaseDescription"
                    | "DefaultUserName"
                    | "CustomIcons"
                    | "MemoryProtection"
                    | "CustomData" => {}
                    "RecycleBinUUID" | "EntryTemplatesGroup" => {
                        return Err(DatabaseError::InvalidFormat(format!(
                            "Meta/{name} value is empty"
                        )))
                    }
                    _ => {
                        db.contains_unsupported_xml = true;
                        db.xml_extensions.meta.push(preserve_empty_element(e)?);
                    }
                }
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of Meta element".into(),
                ))
            }
            _ => {}
        }
    }
}

fn read_memory_protection<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    db: &mut Database,
    inner_stream: &mut dyn InnerStreamCipher,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => {
                let name = tag(&e);
                match name.as_str() {
                    "ProtectTitle" => {
                        db.memory_protection.protect_title =
                            bool_from_xml(&read_text_content(reader, buf)?, "ProtectTitle")?
                    }
                    "ProtectUserName" => {
                        db.memory_protection.protect_username =
                            bool_from_xml(&read_text_content(reader, buf)?, "ProtectUserName")?
                    }
                    "ProtectPassword" => {
                        db.memory_protection.protect_password =
                            bool_from_xml(&read_text_content(reader, buf)?, "ProtectPassword")?
                    }
                    "ProtectURL" => {
                        db.memory_protection.protect_url =
                            bool_from_xml(&read_text_content(reader, buf)?, "ProtectURL")?
                    }
                    "ProtectNotes" => {
                        db.memory_protection.protect_notes =
                            bool_from_xml(&read_text_content(reader, buf)?, "ProtectNotes")?
                    }
                    "AutoEnableVisualHiding" => {
                        db.memory_protection.auto_enable_visual_hiding = bool_from_xml(
                            &read_text_content(reader, buf)?,
                            "AutoEnableVisualHiding",
                        )?
                    }
                    _ => {
                        db.contains_unsupported_xml = true;
                        db.xml_extensions.memory_protection.push(preserve_element(
                            reader,
                            e,
                            inner_stream,
                        )?);
                    }
                }
            }
            Event::End(e) if tag_end(&e) == "MemoryProtection" => return Ok(()),
            Event::Empty(e) => match tag(&e).as_str() {
                "ProtectTitle"
                | "ProtectUserName"
                | "ProtectPassword"
                | "ProtectURL"
                | "ProtectNotes"
                | "AutoEnableVisualHiding" => {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "MemoryProtection/{} value is empty",
                        tag(&e)
                    )))
                }
                _ => {
                    db.contains_unsupported_xml = true;
                    db.xml_extensions
                        .memory_protection
                        .push(preserve_empty_element(e)?);
                }
            },
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of MemoryProtection element".into(),
                ))
            }
            _ => {}
        }
    }
}

fn read_custom_icons<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    db: &mut Database,
    inner_stream: &mut dyn InnerStreamCipher,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) if tag(&e) == "Icon" => {
                let mut uuid = None;
                let mut data = None;
                let mut name = String::new();
                let mut last_modification_time = 0;
                let mut extensions = Vec::new();
                let mut seen = std::collections::HashSet::new();
                loop {
                    buf.clear();
                    match reader.read_event_into(buf)? {
                        Event::Start(icon) => {
                            let icon_tag = tag(&icon);
                            if matches!(
                                icon_tag.as_str(),
                                "UUID" | "Data" | "Name" | "LastModificationTime"
                            ) && !seen.insert(icon_tag.clone())
                            {
                                return Err(DatabaseError::InvalidFormat(format!(
                                    "duplicate custom icon {icon_tag} element"
                                )));
                            }
                            match icon_tag.as_str() {
                                "UUID" => {
                                    uuid = Some(required_uuid_from_b64(
                                        &read_text_content(reader, buf)?,
                                        "custom icon",
                                    )?)
                                }
                                "Data" => {
                                    let encoded = read_text_content(reader, buf)?;
                                    data = Some(
                                        base64::engine::general_purpose::STANDARD
                                            .decode(encoded.trim())
                                            .map_err(|err| {
                                                DatabaseError::InvalidFormat(format!(
                                                    "invalid custom icon data: {err}"
                                                ))
                                            })?,
                                    );
                                }
                                "Name" => name = read_text_content(reader, buf)?,
                                "LastModificationTime" => {
                                    last_modification_time =
                                        date_from_xml(&read_text_content(reader, buf)?)?
                                            .as_millis()
                                            .ok_or_else(|| {
                                                DatabaseError::InvalidFormat(
                                                    "custom icon modification time is empty".into(),
                                                )
                                            })?;
                                }
                                _ => {
                                    db.contains_unsupported_xml = true;
                                    extensions.push(preserve_element(reader, icon, inner_stream)?);
                                }
                            }
                        }
                        Event::End(icon) if tag_end(&icon) == "Icon" => break,
                        Event::Empty(icon) => {
                            let icon_tag = tag(&icon);
                            if matches!(
                                icon_tag.as_str(),
                                "UUID" | "Data" | "Name" | "LastModificationTime"
                            ) && !seen.insert(icon_tag.clone())
                            {
                                return Err(DatabaseError::InvalidFormat(format!(
                                    "duplicate custom icon {icon_tag} element"
                                )));
                            }
                            match icon_tag.as_str() {
                                "UUID" => {
                                    return Err(DatabaseError::InvalidFormat(
                                        "custom icon UUID is empty".into(),
                                    ))
                                }
                                "Data" => data = Some(Vec::new()),
                                "Name" => name.clear(),
                                "LastModificationTime" => {
                                    return Err(DatabaseError::InvalidFormat(
                                        "custom icon modification time is empty".into(),
                                    ))
                                }
                                _ => {
                                    db.contains_unsupported_xml = true;
                                    extensions.push(preserve_empty_element(icon)?);
                                }
                            }
                        }
                        Event::Eof => {
                            return Err(DatabaseError::InvalidFormat(
                                "unexpected end of custom icon".into(),
                            ))
                        }
                        _ => {}
                    }
                }
                let uuid = uuid.ok_or_else(|| {
                    DatabaseError::InvalidFormat("custom icon is missing UUID".into())
                })?;
                let data = data.ok_or_else(|| {
                    DatabaseError::InvalidFormat("custom icon is missing Data".into())
                })?;
                if db.custom_icons.contains_key(&uuid) {
                    return Err(DatabaseError::InvalidFormat(
                        "duplicate custom icon UUID".into(),
                    ));
                }
                if !extensions.is_empty() {
                    db.xml_extensions.custom_icon.insert(uuid, extensions);
                }
                db.custom_icons.insert(
                    uuid,
                    IconImageCustom {
                        uuid,
                        data,
                        name,
                        last_modification_time,
                    },
                );
            }
            Event::End(e) if tag_end(&e) == "CustomIcons" => return Ok(()),
            Event::Start(e) => {
                db.contains_unsupported_xml = true;
                db.xml_extensions
                    .custom_icons
                    .push(preserve_element(reader, e, inner_stream)?);
            }
            Event::Empty(e) if tag(&e) == "Icon" => {
                return Err(DatabaseError::InvalidFormat(
                    "custom Icon element is missing UUID and Data".into(),
                ))
            }
            Event::Empty(e) => {
                db.contains_unsupported_xml = true;
                db.xml_extensions
                    .custom_icons
                    .push(preserve_empty_element(e)?);
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of CustomIcons element".into(),
                ))
            }
            _ => {}
        }
    }
}
