//! KDBX XML reader.

mod data;
mod entry;
mod group;
mod meta;

use super::helpers::*;

/// KDBX XML reader.
pub struct KdbxXmlReader;

impl KdbxXmlReader {
    /// Parse a KDBX XML string into a Database.
    /// `inner_stream` is used to decrypt protected field values.
    pub fn read(xml: &str, inner_stream: &mut dyn InnerStreamCipher) -> DatabaseResult<Database> {
        Self::read_with_binaries(xml, inner_stream, &[])
    }

    /// Parse XML and resolve KDBX4 inner-header binary references.
    pub fn read_with_binaries(
        xml: &str,
        inner_stream: &mut dyn InnerStreamCipher,
        binaries: &[(Vec<u8>, bool)],
    ) -> DatabaseResult<Database> {
        validate_nesting(xml)?;
        let mut reader = Reader::from_str(xml);
        let mut buf = Vec::new();
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
                        read_root(&mut reader, &mut db, inner_stream, binaries, &mut buf)?
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

fn validate_nesting(xml: &str) -> DatabaseResult<()> {
    let mut reader = Reader::from_str(xml);
    let mut depth = 0usize;
    loop {
        match reader.read_event()? {
            Event::Start(_) => {
                depth = depth.checked_add(1).ok_or_else(|| {
                    DatabaseError::InvalidFormat("XML nesting depth overflow".into())
                })?;
                if depth > crate::kdbx::limits::MAX_XML_NESTING_DEPTH {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "XML nesting exceeds {} levels",
                        crate::kdbx::limits::MAX_XML_NESTING_DEPTH
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
    binaries: &[(Vec<u8>, bool)],
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
