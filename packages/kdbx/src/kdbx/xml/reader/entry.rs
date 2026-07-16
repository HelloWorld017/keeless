use super::super::helpers::*;

pub(super) fn read_entry<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    db: &mut Database,
    inner_stream: &mut dyn InnerStreamCipher,
    binaries: &[(Vec<u8>, bool)],
    buf: &mut Vec<u8>,
) -> DatabaseResult<Entry> {
    let mut entry = Entry::new(NodeId::new_uuid());

    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(ref event) => match tag(event).as_str() {
                "UUID" => {
                    let value = read_text_content(reader, buf)?;
                    entry.id = NodeId::from_uuid(required_uuid_from_b64(&value, "entry")?);
                }
                "IconID" => {
                    let id = read_text_content(reader, buf)?;
                    entry.icon =
                        IconImage::Standard(IconImageStandard::new(icon_id_from_str(&id)?));
                }
                "CustomIconUUID" => {
                    let value = read_text_content(reader, buf)?;
                    entry.custom_icon_uuid = if value.is_empty() {
                        None
                    } else {
                        Some(required_uuid_from_b64(&value, "entry custom icon")?)
                    };
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
                "String" => read_entry_string(
                    reader,
                    &mut entry,
                    inner_stream,
                    &mut db.contains_unsupported_xml,
                    buf,
                )?,
                "Binary" => read_entry_binary(
                    reader,
                    &mut entry,
                    binaries,
                    &mut db.contains_unsupported_xml,
                    buf,
                )?,
                "AutoType" => {
                    read_auto_type(reader, &mut entry, &mut db.contains_unsupported_xml, buf)?
                }
                "Times" => read_entry_times(reader, &mut entry, db, buf)?,
                "History" => read_history(reader, &mut entry, db, inner_stream, binaries, buf)?,
                "CustomData" => super::data::read_custom_data(
                    reader,
                    &mut entry.custom_data,
                    &mut db.contains_unsupported_xml,
                    buf,
                )?,
                _ => {
                    db.contains_unsupported_xml = true;
                    skip_element(reader, event.name().as_ref())?;
                }
            },
            Event::End(event) if tag_end(&event) == "Entry" => break,
            Event::Empty(event) => match tag(&event).as_str() {
                "UUID" | "CustomIconUUID" | "ForegroundColor" | "BackgroundColor"
                | "OverrideURL" | "Tags" | "History" | "CustomData" => {}
                _ => db.contains_unsupported_xml = true,
            },
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(entry)
}

fn read_entry_string<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    entry: &mut Entry,
    inner_stream: &mut dyn InnerStreamCipher,
    unsupported: &mut bool,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    let mut key = String::new();
    let mut value = String::new();
    let mut protected = false;

    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => match tag(&e).as_str() {
                "Key" => key = read_text_content(reader, buf)?,
                "Value" => {
                    for attr in e.attributes() {
                        let attr = attr?;
                        if attr.key.as_ref() == b"Protected" {
                            let value = std::str::from_utf8(&attr.value).map_err(|err| {
                                DatabaseError::InvalidFormat(format!(
                                    "invalid Protected attribute: {err}"
                                ))
                            })?;
                            protected = value == "True";
                        }
                    }
                    value = read_text_content(reader, buf)?;
                    if protected && !value.is_empty() {
                        let mut bytes = base64::engine::general_purpose::STANDARD
                            .decode(value.trim())
                            .map_err(|err| {
                                DatabaseError::InvalidFormat(format!(
                                    "invalid protected value base64: {err}"
                                ))
                            })?;
                        inner_stream.process(&mut bytes);
                        value = String::from_utf8(bytes).map_err(|err| {
                            DatabaseError::InvalidFormat(format!(
                                "protected value is not UTF-8: {err}"
                            ))
                        })?;
                    }
                }
                _ => {
                    *unsupported = true;
                    skip_element(reader, e.name().as_ref())?;
                }
            },
            Event::End(e) if tag_end(&e) == "String" => break,
            Event::Empty(e) if tag(&e) == "Value" => {
                for attr in e.attributes() {
                    let attr = attr?;
                    if attr.key.as_ref() == b"Protected" {
                        let value = std::str::from_utf8(&attr.value).map_err(|err| {
                            DatabaseError::InvalidFormat(format!(
                                "invalid Protected attribute: {err}"
                            ))
                        })?;
                        protected = value == "True";
                    }
                }
            }
            Event::Empty(_) => *unsupported = true,
            Event::Eof => break,
            _ => {}
        }
    }

    match key.as_str() {
        "Title" => {
            entry.title = value;
            entry.title_is_protected = protected;
        }
        "UserName" => {
            entry.username = protected_string(&value, protected);
        }
        "Password" => {
            entry.password = protected_string(&value, protected);
        }
        "URL" => {
            entry.url = value;
            entry.url_is_protected = protected;
        }
        "Notes" => {
            entry.notes = protected_string(&value, protected);
        }
        _ => {
            entry.custom_fields.push(EntryField {
                name: key,
                value: protected_string(&value, protected),
                is_protected: protected,
            });
        }
    }
    Ok(())
}

fn protected_string(value: &str, protected: bool) -> ProtectedString {
    if protected {
        ProtectedString::new_protected(value)
    } else {
        ProtectedString::new_plain(value)
    }
}

fn read_entry_binary<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    entry: &mut Entry,
    binaries: &[(Vec<u8>, bool)],
    unsupported: &mut bool,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    let mut key = String::new();
    let mut value = Vec::new();
    let mut is_protected = false;

    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => match tag(&e).as_str() {
                "Key" => key = read_text_content(reader, buf)?,
                "Value" => {
                    let mut reference = None;
                    for attr in e.attributes() {
                        let attr = attr?;
                        if attr.key.as_ref() == b"Ref" {
                            reference = Some(parse_binary_reference(&attr.value)?);
                        }
                    }
                    let text = read_text_content(reader, buf)?;
                    if let Some(index) = reference {
                        let binary = binary_at(binaries, index)?;
                        value = binary.0.clone();
                        is_protected = binary.1;
                    } else {
                        value = base64::engine::general_purpose::STANDARD
                            .decode(text.trim())
                            .map_err(|err| {
                                DatabaseError::InvalidFormat(format!(
                                    "invalid binary base64: {err}"
                                ))
                            })?;
                    }
                }
                _ => {
                    *unsupported = true;
                    skip_element(reader, e.name().as_ref())?;
                }
            },
            Event::Empty(e) if tag(&e) == "Value" => {
                let attr = e
                    .attributes()
                    .find(|attr| {
                        attr.as_ref()
                            .map(|attr| attr.key.as_ref() == b"Ref")
                            .unwrap_or(false)
                    })
                    .ok_or_else(|| {
                        DatabaseError::InvalidFormat("empty binary Value has no Ref".into())
                    })??;
                let binary = binary_at(binaries, parse_binary_reference(&attr.value)?)?;
                value = binary.0.clone();
                is_protected = binary.1;
            }
            Event::End(e) if tag_end(&e) == "Binary" => break,
            Event::Empty(_) => *unsupported = true,
            Event::Eof => break,
            _ => {}
        }
    }

    if !key.is_empty() {
        entry.binaries.push(EntryBinary {
            name: key,
            data: value,
            is_protected,
        });
    }
    Ok(())
}

fn parse_binary_reference(value: &[u8]) -> DatabaseResult<usize> {
    std::str::from_utf8(value)
        .map_err(|err| DatabaseError::InvalidFormat(format!("invalid binary Ref: {err}")))?
        .parse::<usize>()
        .map_err(|err| DatabaseError::InvalidFormat(format!("invalid binary Ref: {err}")))
}

fn binary_at(binaries: &[(Vec<u8>, bool)], index: usize) -> DatabaseResult<&(Vec<u8>, bool)> {
    binaries
        .get(index)
        .ok_or_else(|| DatabaseError::InvalidFormat(format!("binary Ref {index} is out of range")))
}

fn read_auto_type<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    entry: &mut Entry,
    unsupported: &mut bool,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => match tag(&e).as_str() {
                "Enabled" => entry.auto_type.enabled = read_text_content(reader, buf)? != "False",
                "DefaultSequence" => {
                    entry.auto_type.default_sequence = read_text_content(reader, buf)?
                }
                "Association" => read_auto_type_association(reader, entry, unsupported, buf)?,
                _ => {
                    *unsupported = true;
                    skip_element(reader, e.name().as_ref())?;
                }
            },
            Event::End(e) if tag_end(&e) == "AutoType" => return Ok(()),
            Event::Empty(e) => match tag(&e).as_str() {
                "Enabled" | "DefaultSequence" => {}
                _ => *unsupported = true,
            },
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

fn read_auto_type_association<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    entry: &mut Entry,
    unsupported: &mut bool,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    let mut association = AutoTypeAssociation {
        window_title: String::new(),
        keystroke_sequence: String::new(),
    };
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => match tag(&e).as_str() {
                "Window" => association.window_title = read_text_content(reader, buf)?,
                "KeystrokeSequence" => {
                    association.keystroke_sequence = read_text_content(reader, buf)?
                }
                _ => {
                    *unsupported = true;
                    skip_element(reader, e.name().as_ref())?;
                }
            },
            Event::End(e) if tag_end(&e) == "Association" => {
                entry.auto_type.associations.push(association);
                return Ok(());
            }
            Event::Empty(e) => match tag(&e).as_str() {
                "Window" | "KeystrokeSequence" => {}
                _ => *unsupported = true,
            },
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

fn read_entry_times<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    entry: &mut Entry,
    db: &mut Database,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => match tag(&e).as_str() {
                "CreationTime" => {
                    entry.creation_time = date_from_xml(&read_text_content(reader, buf)?)?
                }
                "LastModificationTime" => {
                    entry.last_modification_time = date_from_xml(&read_text_content(reader, buf)?)?
                }
                "LastAccessTime" => {
                    entry.last_access_time = date_from_xml(&read_text_content(reader, buf)?)?
                }
                "LocationChanged" => {
                    entry.location_changed = date_from_xml(&read_text_content(reader, buf)?)?
                }
                "ExpiryTime" => {
                    entry.expiry_time = date_from_xml(&read_text_content(reader, buf)?)?
                }
                "Expires" => entry.expires = read_text_content(reader, buf)? == "True",
                "UsageCount" => {
                    entry.usage_count = read_text_content(reader, buf)?.parse().map_err(|err| {
                        DatabaseError::InvalidFormat(format!("invalid entry usage count: {err}"))
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

fn read_history<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    entry: &mut Entry,
    db: &mut Database,
    inner_stream: &mut dyn InnerStreamCipher,
    binaries: &[(Vec<u8>, bool)],
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) if tag(&e) == "Entry" => {
                let history_entry = read_entry(reader, db, inner_stream, binaries, buf)?;
                entry.history.push(history_entry);
            }
            Event::End(e) if tag_end(&e) == "History" => return Ok(()),
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
