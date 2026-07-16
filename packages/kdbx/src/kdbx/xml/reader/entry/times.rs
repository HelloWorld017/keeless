use crate::kdbx::xml::helpers::*;

pub(super) fn read_entry_times<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    entry: &mut Entry,
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
                        "duplicate entry Times/{name} element"
                    )));
                }
                match name.as_str() {
                    "CreationTime" => {
                        entry.creation_time = date_from_xml(&read_text_content(reader, buf)?)?
                    }
                    "LastModificationTime" => {
                        entry.last_modification_time =
                            date_from_xml(&read_text_content(reader, buf)?)?
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
                    "Expires" => {
                        entry.expires =
                            bool_from_xml(&read_text_content(reader, buf)?, "entry Expires")?
                    }
                    "UsageCount" => {
                        entry.usage_count =
                            read_text_content(reader, buf)?.parse().map_err(|err| {
                                DatabaseError::InvalidFormat(format!(
                                    "invalid entry usage count: {err}"
                                ))
                            })?
                    }
                    _ => {
                        db.contains_unsupported_xml = true;
                        entry
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
                        "duplicate entry Times/{name} element"
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
                            "entry Times/{name} value is empty"
                        )))
                    }
                    _ => {
                        db.contains_unsupported_xml = true;
                        entry.xml_extensions.times.push(preserve_empty_element(e)?);
                    }
                }
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of entry Times element".into(),
                ))
            }
            _ => {}
        }
    }
}
