use crate::kdbx::xml::helpers::*;

pub(super) fn read_entry_binary<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    entry: &mut Entry,
    binaries: &[(Vec<u8>, bool)],
    inner_stream: &mut dyn InnerStreamCipher,
    unsupported: &mut bool,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    let mut key = None;
    let mut value: Option<Zeroizing<Vec<u8>>> = None;
    let mut is_protected = false;
    let mut extensions = Vec::new();
    let mut seen = std::collections::HashSet::new();

    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => {
                let name = tag(&e);
                if matches!(name.as_str(), "Key" | "Value") && !seen.insert(name.clone()) {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "duplicate Binary/{name} element"
                    )));
                }
                match name.as_str() {
                    "Key" => key = Some(read_text_content(reader, buf)?),
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
                            value = Some(Zeroizing::new(binary.0.clone()));
                            is_protected = binary.1;
                        } else {
                            value = Some(Zeroizing::new(
                                base64::engine::general_purpose::STANDARD
                                    .decode(text.trim())
                                    .map_err(|err| {
                                        DatabaseError::InvalidFormat(format!(
                                            "invalid binary base64: {err}"
                                        ))
                                    })?,
                            ));
                        }
                    }
                    _ => {
                        *unsupported = true;
                        extensions.push(preserve_element(reader, e, inner_stream)?);
                    }
                }
            }
            Event::Empty(e) if tag(&e) == "Value" => {
                if !seen.insert("Value".to_string()) {
                    return Err(DatabaseError::InvalidFormat(
                        "duplicate Binary/Value element".into(),
                    ));
                }
                let reference = e
                    .attributes()
                    .find(|attr| {
                        attr.as_ref()
                            .map(|attr| attr.key.as_ref() == b"Ref")
                            .unwrap_or(false)
                    })
                    .transpose()?;
                if let Some(reference) = reference {
                    let binary = binary_at(binaries, parse_binary_reference(&reference.value)?)?;
                    value = Some(Zeroizing::new(binary.0.clone()));
                    is_protected = binary.1;
                } else {
                    value = Some(Zeroizing::new(Vec::new()));
                }
            }
            Event::Empty(e) if tag(&e) == "Key" => {
                if !seen.insert("Key".to_string()) {
                    return Err(DatabaseError::InvalidFormat(
                        "duplicate Binary/Key element".into(),
                    ));
                }
                key = Some(String::new());
            }
            Event::End(e) if tag_end(&e) == "Binary" => break,
            Event::Empty(e) => {
                *unsupported = true;
                extensions.push(preserve_empty_element(e)?);
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of Binary element".into(),
                ))
            }
            _ => {}
        }
    }

    let key =
        key.ok_or_else(|| DatabaseError::InvalidFormat("Binary element is missing Key".into()))?;
    let value = value
        .ok_or_else(|| DatabaseError::InvalidFormat("Binary element is missing Value".into()))?;
    if entry.xml_extensions.binaries.contains_key(&key) {
        return Err(DatabaseError::InvalidFormat(format!(
            "duplicate entry Binary key: {key}"
        )));
    }
    entry
        .xml_extensions
        .binaries
        .insert(key.clone(), extensions);
    entry.binaries.push(EntryBinary {
        name: key,
        data: value.to_vec(),
        is_protected,
    });
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
