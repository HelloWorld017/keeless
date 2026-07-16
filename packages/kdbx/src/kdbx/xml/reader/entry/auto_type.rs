use crate::kdbx::xml::helpers::*;

pub(super) fn read_auto_type<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    entry: &mut Entry,
    inner_stream: &mut dyn InnerStreamCipher,
    unsupported: &mut bool,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    let mut seen = std::collections::HashSet::new();
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => {
                let name = tag(&e);
                if matches!(name.as_str(), "Enabled" | "DefaultSequence")
                    && !seen.insert(name.clone())
                {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "duplicate AutoType/{name} element"
                    )));
                }
                match name.as_str() {
                    "Enabled" => {
                        entry.auto_type.enabled =
                            bool_from_xml(&read_text_content(reader, buf)?, "AutoType Enabled")?
                    }
                    "DefaultSequence" => {
                        entry.auto_type.default_sequence = read_text_content(reader, buf)?
                    }
                    "Association" => {
                        read_auto_type_association(reader, entry, inner_stream, unsupported, buf)?
                    }
                    _ => {
                        *unsupported = true;
                        entry.xml_extensions.auto_type.push(preserve_element(
                            reader,
                            e,
                            inner_stream,
                        )?);
                    }
                }
            }
            Event::End(e) if tag_end(&e) == "AutoType" => return Ok(()),
            Event::Empty(e) => match tag(&e).as_str() {
                "Enabled" => {
                    return Err(DatabaseError::InvalidFormat(
                        "AutoType Enabled value is empty".into(),
                    ))
                }
                "DefaultSequence" => {
                    if !seen.insert("DefaultSequence".to_string()) {
                        return Err(DatabaseError::InvalidFormat(
                            "duplicate AutoType/DefaultSequence element".into(),
                        ));
                    }
                    entry.auto_type.default_sequence.clear();
                }
                "Association" => {
                    return Err(DatabaseError::InvalidFormat(
                        "empty AutoType Association".into(),
                    ))
                }
                _ => {
                    *unsupported = true;
                    entry
                        .xml_extensions
                        .auto_type
                        .push(preserve_empty_element(e)?);
                }
            },
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of AutoType element".into(),
                ))
            }
            _ => {}
        }
    }
}

fn read_auto_type_association<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    entry: &mut Entry,
    inner_stream: &mut dyn InnerStreamCipher,
    unsupported: &mut bool,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    let mut window_title = None;
    let mut keystroke_sequence = None;
    let mut extensions = Vec::new();
    let mut seen = std::collections::HashSet::new();
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => {
                let name = tag(&e);
                if matches!(name.as_str(), "Window" | "KeystrokeSequence")
                    && !seen.insert(name.clone())
                {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "duplicate Association/{name} element"
                    )));
                }
                match name.as_str() {
                    "Window" => window_title = Some(read_text_content(reader, buf)?),
                    "KeystrokeSequence" => {
                        keystroke_sequence = Some(read_text_content(reader, buf)?)
                    }
                    _ => {
                        *unsupported = true;
                        extensions.push(preserve_element(reader, e, inner_stream)?);
                    }
                }
            }
            Event::End(e) if tag_end(&e) == "Association" => {
                entry.auto_type.associations.push(AutoTypeAssociation {
                    window_title: window_title.ok_or_else(|| {
                        DatabaseError::InvalidFormat(
                            "AutoType Association is missing Window".into(),
                        )
                    })?,
                    keystroke_sequence: keystroke_sequence.ok_or_else(|| {
                        DatabaseError::InvalidFormat(
                            "AutoType Association is missing KeystrokeSequence".into(),
                        )
                    })?,
                });
                entry.xml_extensions.associations.push(extensions);
                return Ok(());
            }
            Event::Empty(e) => match tag(&e).as_str() {
                "Window" => {
                    if !seen.insert("Window".to_string()) {
                        return Err(DatabaseError::InvalidFormat(
                            "duplicate Association/Window element".into(),
                        ));
                    }
                    window_title = Some(String::new());
                }
                "KeystrokeSequence" => {
                    if !seen.insert("KeystrokeSequence".to_string()) {
                        return Err(DatabaseError::InvalidFormat(
                            "duplicate Association/KeystrokeSequence element".into(),
                        ));
                    }
                    keystroke_sequence = Some(String::new());
                }
                _ => {
                    *unsupported = true;
                    extensions.push(preserve_empty_element(e)?);
                }
            },
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of AutoType Association".into(),
                ))
            }
            _ => {}
        }
    }
}
