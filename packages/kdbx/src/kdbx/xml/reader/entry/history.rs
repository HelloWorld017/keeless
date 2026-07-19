use super::super::BinaryReferences;
use crate::kdbx::xml::helpers::*;

pub(super) fn read_history<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    entry: &mut Entry,
    db: &mut Database,
    inner_stream: &mut dyn InnerStreamCipher,
    binaries: &BinaryReferences<'_>,
    buf: &mut Vec<u8>,
) -> DatabaseResult<()> {
    loop {
        buf.clear();
        match reader.read_event_into(buf)? {
            Event::Start(e) if tag(&e) == "Entry" => {
                let history_entry = super::read_entry(reader, db, inner_stream, binaries, buf)?;
                entry.history.push(history_entry);
            }
            Event::End(e) if tag_end(&e) == "History" => return Ok(()),
            Event::Start(e) => {
                db.contains_unsupported_xml = true;
                entry
                    .xml_extensions
                    .history
                    .push(preserve_element(reader, e, inner_stream)?);
            }
            Event::Empty(e) if tag(&e) == "Entry" => {
                return Err(DatabaseError::InvalidFormat(
                    "empty history Entry is missing UUID".into(),
                ))
            }
            Event::Empty(e) => {
                db.contains_unsupported_xml = true;
                entry
                    .xml_extensions
                    .history
                    .push(preserve_empty_element(e)?);
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of History element".into(),
                ))
            }
            _ => {}
        }
    }
}
