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

        loop {
            buf.clear();
            match reader.read_event_into(&mut buf)? {
                Event::Start(ref e) => match tag(e).as_str() {
                    "Meta" => meta::read_meta(&mut reader, &mut db, &mut buf)?,
                    "Root" => read_root(&mut reader, &mut db, inner_stream, binaries, &mut buf)?,
                    "KeePassFile" => {}
                    _ => {
                        db.contains_unsupported_xml = true;
                        skip_element(&mut reader, e.name().as_ref())?;
                    }
                },
                Event::End(_) | Event::Eof => break,
                Event::Empty(_) => db.contains_unsupported_xml = true,
                _ => {}
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
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) => match tag(&e).as_str() {
                "Group" => {
                    let root_group = group::read_group(reader, db, inner_stream, binaries, buf)?;
                    let root_id = root_group.id;
                    db.groups.insert(root_id, root_group);
                    db.root_group_id = Some(root_id);
                }
                "DeletedObjects" => data::read_deleted_objects(reader, db, buf)?,
                _ => {
                    db.contains_unsupported_xml = true;
                    skip_element(reader, e.name().as_ref())?;
                }
            },
            Event::End(e) if tag_end(&e) == "Root" => return Ok(()),
            Event::Empty(e) if tag(&e) != "DeletedObjects" => db.contains_unsupported_xml = true,
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}
