//! KDBX XML reader.

mod data;
mod entry;
mod group;
mod meta;

use super::helpers::*;
use crate::crypto::compression::decompress;
use crate::kdbx::limits::{MAX_DECOMPRESSED_PAYLOAD_SIZE, MAX_XML_NESTING_DEPTH};

enum BinaryReferences<'a> {
    ById(&'a std::collections::HashMap<usize, (Vec<u8>, bool)>),
    ByIndex(&'a [(Vec<u8>, bool)]),
}

impl BinaryReferences<'_> {
    fn get(&self, reference: usize) -> Option<&(Vec<u8>, bool)> {
        match self {
            Self::ById(binaries) => binaries.get(&reference),
            Self::ByIndex(binaries) => binaries.get(reference),
        }
    }
}

/// KDBX XML reader.
pub struct KdbxXmlReader;

impl KdbxXmlReader {
    /// Parse a KDBX XML string into a Database.
    /// `inner_stream` is used to decrypt protected field values.
    pub fn read(xml: &str, inner_stream: &mut dyn InnerStreamCipher) -> DatabaseResult<Database> {
        let binaries = read_meta_binaries(xml)?;
        Self::read_internal(xml, inner_stream, BinaryReferences::ById(&binaries))
    }

    /// Parse XML and resolve KDBX4 inner-header binary references.
    pub fn read_with_binaries(
        xml: &str,
        inner_stream: &mut dyn InnerStreamCipher,
        binaries: &[(Vec<u8>, bool)],
    ) -> DatabaseResult<Database> {
        Self::read_internal(xml, inner_stream, BinaryReferences::ByIndex(binaries))
    }

    fn read_internal(
        xml: &str,
        inner_stream: &mut dyn InnerStreamCipher,
        binaries: BinaryReferences<'_>,
    ) -> DatabaseResult<Database> {
        validate_nesting(xml)?;
        let mut reader = Reader::from_str(xml);
        let mut buf = Zeroizing::new(Vec::new());
        let mut db = Database::default();
        let mut saw_keepass_file = false;
        let mut saw_meta = false;
        let mut saw_root = false;

        loop {
            buf.clear();
            match reader.read_event_into(&mut buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "KeePassFile" if !saw_keepass_file => saw_keepass_file = true,
                    "KeePassFile" => {
                        return Err(DatabaseError::InvalidFormat(
                            "duplicate KeePassFile element".into(),
                        ))
                    }
                    "Meta" if saw_keepass_file && !saw_meta && !saw_root => {
                        saw_meta = true;
                        meta::read_meta(&mut reader, &mut db, inner_stream, &mut buf)?
                    }
                    "Meta" => {
                        return Err(DatabaseError::InvalidFormat(
                            "duplicate or misplaced Meta element".into(),
                        ))
                    }
                    "Root" if saw_keepass_file && saw_meta && !saw_root => {
                        saw_root = true;
                        read_root(&mut reader, &mut db, inner_stream, &binaries, &mut buf)?
                    }
                    "Root" => {
                        return Err(DatabaseError::InvalidFormat(
                            "duplicate or misplaced Root element".into(),
                        ))
                    }
                    _ if !saw_keepass_file => {
                        return Err(DatabaseError::InvalidFormat(
                            "XML root element must be KeePassFile".into(),
                        ))
                    }
                    _ => {
                        db.contains_unsupported_xml = true;
                        db.xml_extensions.keepass_file.push(preserve_element(
                            &mut reader,
                            e,
                            inner_stream,
                        )?);
                    }
                },
                Event::End(e) if tag_end(&e) == "KeePassFile" => break,
                Event::End(_) => {
                    return Err(DatabaseError::InvalidFormat(
                        "unexpected XML end element".into(),
                    ))
                }
                Event::Empty(e) => match tag(&e).as_str() {
                    "KeePassFile" | "Meta" | "Root" => {
                        return Err(DatabaseError::InvalidFormat(format!(
                            "{} element cannot be empty",
                            tag(&e)
                        )))
                    }
                    _ => {
                        db.contains_unsupported_xml = true;
                        db.xml_extensions
                            .keepass_file
                            .push(preserve_empty_element(e)?);
                    }
                },
                Event::DocType(_) => {
                    return Err(DatabaseError::InvalidFormat(
                        "XML DTDs are not supported".into(),
                    ))
                }
                Event::Eof => break,
                _ => {}
            }
        }

        if !saw_keepass_file || !saw_meta || !saw_root || db.root_group_id.is_none() {
            return Err(DatabaseError::InvalidFormat(
                "XML is missing KeePassFile, Meta, Root, or root Group".into(),
            ));
        }

        loop {
            match reader.read_event()? {
                Event::Eof => break,
                Event::Text(text) if text.as_ref().iter().all(u8::is_ascii_whitespace) => {}
                Event::Comment(_) | Event::PI(_) => {}
                _ => {
                    return Err(DatabaseError::InvalidFormat(
                        "unexpected content after KeePassFile".into(),
                    ))
                }
            }
        }

        Ok(db)
    }
}

fn read_meta_binaries(
    xml: &str,
) -> DatabaseResult<std::collections::HashMap<usize, (Vec<u8>, bool)>> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Zeroizing::new(Vec::new());
    let mut binaries = std::collections::HashMap::new();
    let mut total_size = 0usize;
    let mut depth = 0usize;
    let mut in_meta = false;
    let mut in_binaries = false;

    loop {
        buf.clear();
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) if in_binaries && depth == 3 && tag(&e) == "Binary" => {
                let (id, compressed) = read_binary_attributes(&e)?;
                let encoded = read_meta_binary_text(&mut reader, &mut buf)?;
                let data = decode_meta_binary(&encoded, compressed)?;
                add_meta_binary(&mut binaries, &mut total_size, id, data)?;
            }
            Event::Empty(e) if in_binaries && depth == 3 && tag(&e) == "Binary" => {
                let (id, compressed) = read_binary_attributes(&e)?;
                let data = decode_meta_binary("", compressed)?;
                add_meta_binary(&mut binaries, &mut total_size, id, data)?;
            }
            Event::Start(e) => {
                let name = tag(&e);
                if depth == 1 && name == "Meta" {
                    in_meta = true;
                } else if in_meta && depth == 2 && name == "Binaries" {
                    in_binaries = true;
                }
                depth = depth.checked_add(1).ok_or_else(|| {
                    DatabaseError::InvalidFormat("XML nesting depth overflow".into())
                })?;
            }
            Event::End(e) => {
                depth = depth.checked_sub(1).ok_or_else(|| {
                    DatabaseError::InvalidFormat("Unbalanced XML end element".into())
                })?;
                let name = tag_end(&e);
                if in_binaries && depth == 2 && name == "Binaries" {
                    in_binaries = false;
                } else if in_meta && depth == 1 && name == "Meta" {
                    in_meta = false;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(binaries)
}

fn read_meta_binary_text<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    buf: &mut Vec<u8>,
) -> DatabaseResult<String> {
    let mut encoded = String::new();
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Text(text) => encoded.push_str(
                &text
                    .unescape()
                    .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?,
            ),
            Event::CData(value) => {
                encoded.push_str(std::str::from_utf8(value.as_ref()).map_err(|err| {
                    DatabaseError::InvalidFormat(format!("invalid binary base64: {err}"))
                })?)
            }
            Event::End(e) if tag_end(&e) == "Binary" => return Ok(encoded),
            Event::Start(_) | Event::Empty(_) => {
                return Err(DatabaseError::InvalidFormat(
                    "Meta/Binaries/Binary contains nested elements".into(),
                ))
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of Meta/Binaries/Binary element".into(),
                ))
            }
            _ => {}
        }
    }
}

fn read_binary_attributes(e: &BytesStart<'_>) -> DatabaseResult<(usize, bool)> {
    let mut id = None;
    let mut compressed = None;
    for attribute in e.attributes() {
        let attribute = attribute?;
        match attribute.key.as_ref() {
            b"ID" => {
                if id.is_some() {
                    return Err(DatabaseError::InvalidFormat(
                        "duplicate Meta/Binaries/Binary ID attribute".into(),
                    ));
                }
                let value = std::str::from_utf8(&attribute.value).map_err(|err| {
                    DatabaseError::InvalidFormat(format!("invalid binary ID: {err}"))
                })?;
                id = Some(value.parse::<u32>().map_err(|err| {
                    DatabaseError::InvalidFormat(format!("invalid binary ID: {err}"))
                })? as usize);
            }
            b"Compressed" => {
                if compressed.is_some() {
                    return Err(DatabaseError::InvalidFormat(
                        "duplicate Meta/Binaries/Binary Compressed attribute".into(),
                    ));
                }
                let value = std::str::from_utf8(&attribute.value).map_err(|err| {
                    DatabaseError::InvalidFormat(format!("invalid Compressed attribute: {err}"))
                })?;
                compressed = Some(bool_from_xml(value, "binary Compressed")?);
            }
            _ => {}
        }
    }

    let id = id
        .ok_or_else(|| DatabaseError::InvalidFormat("Meta/Binaries/Binary is missing ID".into()))?;
    Ok((id, compressed.unwrap_or(false)))
}

fn decode_meta_binary(encoded: &str, compressed: bool) -> DatabaseResult<Vec<u8>> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|err| DatabaseError::InvalidFormat(format!("invalid binary base64: {err}")))?;
    if compressed {
        decompress(&data)
    } else {
        Ok(data)
    }
}

fn add_meta_binary(
    binaries: &mut std::collections::HashMap<usize, (Vec<u8>, bool)>,
    total_size: &mut usize,
    id: usize,
    data: Vec<u8>,
) -> DatabaseResult<()> {
    if binaries.contains_key(&id) {
        return Err(DatabaseError::InvalidFormat(format!(
            "duplicate Meta/Binaries/Binary ID {id}"
        )));
    }
    *total_size = total_size
        .checked_add(data.len())
        .ok_or_else(|| DatabaseError::InvalidFormat("binary pool size overflow".into()))?;
    if *total_size > MAX_DECOMPRESSED_PAYLOAD_SIZE {
        return Err(DatabaseError::InvalidFormat(format!(
            "binary pool exceeds {MAX_DECOMPRESSED_PAYLOAD_SIZE} bytes"
        )));
    }
    binaries.insert(id, (data, false));
    Ok(())
}

fn validate_nesting(xml: &str) -> DatabaseResult<()> {
    let mut reader = Reader::from_str(xml);
    let mut depth = 0usize;
    loop {
        match reader.read_event()? {
            Event::Start(_) => {
                depth = depth.checked_add(1).ok_or_else(|| {
                    DatabaseError::InvalidFormat("XML nesting depth overflow".into())
                })?;
                if depth > MAX_XML_NESTING_DEPTH {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "XML nesting exceeds {} levels",
                        MAX_XML_NESTING_DEPTH
                    )));
                }
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or_else(|| {
                    DatabaseError::InvalidFormat("Unbalanced XML end element".into())
                })?;
            }
            Event::Eof => break,
            Event::DocType(_) => {
                return Err(DatabaseError::InvalidFormat(
                    "XML DTDs are not supported".into(),
                ))
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(DatabaseError::InvalidFormat(
            "Unbalanced XML elements".into(),
        ));
    }
    Ok(())
}

fn read_root<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    db: &mut Database,
    inner_stream: &mut dyn InnerStreamCipher,
    binaries: &BinaryReferences<'_>,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    let mut saw_group = false;
    let mut saw_deleted_objects = false;
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => match tag(&e).as_str() {
                "Group" => {
                    if saw_group {
                        return Err(DatabaseError::InvalidFormat(
                            "Root contains multiple groups".into(),
                        ));
                    }
                    saw_group = true;
                    let root_group = group::read_group(reader, db, inner_stream, binaries, buf)?;
                    let root_id = root_group.id;
                    db.groups.insert(root_id, root_group);
                    db.root_group_id = Some(root_id);
                }
                "DeletedObjects" => {
                    if saw_deleted_objects {
                        return Err(DatabaseError::InvalidFormat(
                            "duplicate DeletedObjects element".into(),
                        ));
                    }
                    saw_deleted_objects = true;
                    data::read_deleted_objects(reader, db, inner_stream, buf)?
                }
                _ => {
                    db.contains_unsupported_xml = true;
                    db.xml_extensions
                        .root
                        .push(preserve_element(reader, e, inner_stream)?);
                }
            },
            Event::End(e) if tag_end(&e) == "Root" => {
                return if saw_group {
                    Ok(())
                } else {
                    Err(DatabaseError::InvalidFormat(
                        "Root is missing its Group".into(),
                    ))
                }
            }
            Event::Empty(e) if tag(&e) == "DeletedObjects" => {
                if saw_deleted_objects {
                    return Err(DatabaseError::InvalidFormat(
                        "duplicate DeletedObjects element".into(),
                    ));
                }
                saw_deleted_objects = true;
            }
            Event::Empty(e) if tag(&e) == "Group" => {
                return Err(DatabaseError::InvalidFormat(
                    "root Group is missing its UUID".into(),
                ))
            }
            Event::Empty(e) => {
                db.contains_unsupported_xml = true;
                db.xml_extensions.root.push(preserve_empty_element(e)?);
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of Root element".into(),
                ))
            }
            _ => {}
        }
    }
}
