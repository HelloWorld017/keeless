use crate::kdbx::xml::helpers::*;

pub(super) fn read_entry_string<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    entry: &mut Entry,
    inner_stream: &mut dyn InnerStreamCipher,
    unsupported: &mut bool,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    let mut key = None;
    let mut value: Option<Zeroizing<String>> = None;
    let mut protected = false;
    let mut extensions = Vec::new();
    let mut seen = std::collections::HashSet::new();

    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => {
                let name = tag(&e);
                if matches!(name.as_str(), "Key" | "Value") && !seen.insert(name.clone()) {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "duplicate String/{name} element"
                    )));
                }
                match name.as_str() {
                    "Key" => key = Some(read_text_content(reader, buf)?),
                    "Value" => {
                        for attr in e.attributes() {
                            let attr = attr?;
                            if attr.key.as_ref() == b"Protected" {
                                let value = std::str::from_utf8(&attr.value).map_err(|err| {
                                    DatabaseError::InvalidFormat(format!(
                                        "invalid Protected attribute: {err}"
                                    ))
                                })?;
                                protected = bool_from_xml(value, "Protected attribute")?;
                            }
                        }
                        let mut parsed_value = Zeroizing::new(read_text_content(reader, buf)?);
                        if protected && !parsed_value.is_empty() {
                            let mut bytes = Zeroizing::new(
                                base64::engine::general_purpose::STANDARD
                                    .decode(parsed_value.trim())
                                    .map_err(|err| {
                                        DatabaseError::InvalidFormat(format!(
                                            "invalid protected value base64: {err}"
                                        ))
                                    })?,
                            );
                            inner_stream.process(&mut bytes);
                            let decrypted =
                                std::str::from_utf8(bytes.as_slice()).map_err(|err| {
                                    DatabaseError::InvalidFormat(format!(
                                        "protected value is not UTF-8: {err}"
                                    ))
                                })?;
                            parsed_value.zeroize();
                            parsed_value.push_str(decrypted);
                        }
                        value = Some(parsed_value);
                    }
                    _ => {
                        *unsupported = true;
                        extensions.push(preserve_element(reader, e, inner_stream)?);
                    }
                }
            }
            Event::End(e) if tag_end(&e) == "String" => break,
            Event::Empty(e) if tag(&e) == "Value" => {
                if !seen.insert("Value".to_string()) {
                    return Err(DatabaseError::InvalidFormat(
                        "duplicate String/Value element".into(),
                    ));
                }
                for attr in e.attributes() {
                    let attr = attr?;
                    if attr.key.as_ref() == b"Protected" {
                        let value = std::str::from_utf8(&attr.value).map_err(|err| {
                            DatabaseError::InvalidFormat(format!(
                                "invalid Protected attribute: {err}"
                            ))
                        })?;
                        protected = bool_from_xml(value, "Protected attribute")?;
                    }
                }
                value = Some(Zeroizing::new(String::new()));
            }
            Event::Empty(e) if tag(&e) == "Key" => {
                if !seen.insert("Key".to_string()) {
                    return Err(DatabaseError::InvalidFormat(
                        "duplicate String/Key element".into(),
                    ));
                }
                key = Some(String::new());
            }
            Event::Empty(e) => {
                *unsupported = true;
                extensions.push(preserve_empty_element(e)?);
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of String element".into(),
                ))
            }
            _ => {}
        }
    }

    let key =
        key.ok_or_else(|| DatabaseError::InvalidFormat("String element is missing Key".into()))?;
    let value = value
        .ok_or_else(|| DatabaseError::InvalidFormat("String element is missing Value".into()))?;
    if entry.xml_extensions.strings.contains_key(&key) {
        return Err(DatabaseError::InvalidFormat(format!(
            "duplicate entry String key: {key}"
        )));
    }
    entry.xml_extensions.strings.insert(key.clone(), extensions);

    match key.as_str() {
        "Title" => {
            entry.title = value.to_string();
            entry.title_is_protected = protected;
        }
        "UserName" => {
            entry.username = protected_string(&value, protected);
        }
        "Password" => {
            entry.password = protected_string(&value, protected);
        }
        "URL" => {
            entry.url = value.to_string();
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
