//! Shared helpers for XML reading/writing.

pub(crate) use base64::Engine;
pub(crate) use quick_xml::events::{
    BytesCData, BytesDecl, BytesEnd, BytesPI, BytesStart, BytesText, Event,
};
pub(crate) use quick_xml::Reader;
pub(crate) use uuid::Uuid;
pub(crate) use zeroize::{Zeroize, Zeroizing};

pub(crate) use crate::crypto::inner_stream::InnerStreamCipher;
pub(crate) use crate::model::core::date::DateInstant;
pub(crate) use crate::model::core::node::NodeId;
pub(crate) use crate::model::core::security::ProtectedString;
pub(crate) use crate::model::db::database::Database;
pub(crate) use crate::model::entry::{AutoTypeAssociation, Entry, EntryBinary, EntryField};
pub(crate) use crate::model::exception::{DatabaseError, DatabaseResult};
pub(crate) use crate::model::group::Group;
pub(crate) use crate::model::meta::icon::{IconImage, IconImageCustom, IconImageStandard};
pub(crate) use crate::model::meta::{CustomData, CustomDataItem, DeletedObject};
pub(crate) use crate::model::xml::{PreservedXmlContent, PreservedXmlElement};

pub(crate) fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD_NO_PAD
}

pub(crate) fn tag(e: &quick_xml::events::BytesStart<'_>) -> String {
    std::str::from_utf8(e.name().as_ref())
        .unwrap_or("")
        .to_string()
}

pub(crate) fn tag_end(e: &quick_xml::events::BytesEnd<'_>) -> String {
    std::str::from_utf8(e.name().as_ref())
        .unwrap_or("")
        .to_string()
}

/// Read text content of the current element. Consumes until End event.
pub(crate) fn read_text_content<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    buf: &mut Vec<u8>,
) -> DatabaseResult<String> {
    loop {
        buf.clear();
        let ev = reader.read_event_into(buf)?;
        match ev {
            Event::Text(t) => {
                return Ok(t
                    .unescape()
                    .map_err(|e| DatabaseError::InvalidFormat(e.to_string()))?
                    .into_owned());
            }
            Event::CData(value) => {
                return std::str::from_utf8(value.as_ref())
                    .map(str::to_owned)
                    .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))
            }
            Event::End(_) => return Ok(String::new()),
            Event::Start(_) | Event::Empty(_) => {
                return Err(DatabaseError::InvalidFormat(
                    "scalar XML element contains nested elements".into(),
                ))
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of scalar XML element".into(),
                ))
            }
            _ => {}
        }
    }
}

/// Capture an unknown element while retaining its semantic XML content.
pub(crate) fn preserve_element<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    start: BytesStart<'_>,
    inner_stream: &mut dyn InnerStreamCipher,
) -> DatabaseResult<PreservedXmlElement> {
    let protected = element_is_protected(&start)?;
    let start_text = std::str::from_utf8(start.as_ref())
        .map_err(|err| DatabaseError::InvalidFormat(format!("invalid XML element: {err}")))?
        .to_string();
    let name = start.name().as_ref().to_vec();
    let name_len = name.len();
    let mut content = Vec::new();
    let mut local_buf = Zeroizing::new(Vec::new());

    loop {
        local_buf.clear();
        match reader.read_event_into(&mut local_buf)? {
            Event::Start(child) => content.push(PreservedXmlContent::Element(preserve_element(
                reader,
                child,
                inner_stream,
            )?)),
            Event::Empty(child) => {
                content.push(PreservedXmlContent::Element(preserve_empty_element(child)?))
            }
            Event::Text(text) if protected => {
                let encoded = text
                    .unescape()
                    .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?;
                let mut value = base64::engine::general_purpose::STANDARD
                    .decode(encoded.trim())
                    .map_err(|err| {
                        DatabaseError::InvalidFormat(format!(
                            "invalid protected extension value base64: {err}"
                        ))
                    })?;
                inner_stream.process(&mut value);
                content.push(PreservedXmlContent::Protected(value));
            }
            Event::Text(text) => content.push(PreservedXmlContent::Text(
                std::str::from_utf8(text.as_ref())
                    .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?
                    .to_string(),
            )),
            Event::CData(value) => content.push(PreservedXmlContent::CData(
                std::str::from_utf8(value.as_ref())
                    .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?
                    .to_string(),
            )),
            Event::Comment(value) => content.push(PreservedXmlContent::Comment(
                std::str::from_utf8(value.as_ref())
                    .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?
                    .to_string(),
            )),
            Event::PI(value) => content.push(PreservedXmlContent::ProcessingInstruction(
                std::str::from_utf8(value.as_ref())
                    .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?
                    .to_string(),
            )),
            Event::End(end) if end.name().as_ref() == name.as_slice() => break,
            Event::End(_) => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected XML end element in extension".into(),
                ))
            }
            Event::DocType(_) | Event::Decl(_) => {
                return Err(DatabaseError::InvalidFormat(
                    "XML declarations and DTDs are not allowed inside elements".into(),
                ))
            }
            Event::Eof => {
                return Err(DatabaseError::InvalidFormat(
                    "unexpected end of XML extension".into(),
                ))
            }
        }
    }

    Ok(PreservedXmlElement {
        start: start_text,
        name_len,
        content,
        empty: false,
    })
}

pub(crate) fn preserve_empty_element(start: BytesStart<'_>) -> DatabaseResult<PreservedXmlElement> {
    let name_len = start.name().as_ref().len();
    let start = std::str::from_utf8(start.as_ref())
        .map_err(|err| DatabaseError::InvalidFormat(format!("invalid XML element: {err}")))?
        .to_string();
    Ok(PreservedXmlElement {
        start,
        name_len,
        content: Vec::new(),
        empty: true,
    })
}

fn element_is_protected(start: &BytesStart<'_>) -> DatabaseResult<bool> {
    for attribute in start.attributes() {
        let attribute = attribute?;
        if attribute.key.as_ref() == b"Protected" {
            let value = std::str::from_utf8(&attribute.value).map_err(|err| {
                DatabaseError::InvalidFormat(format!("invalid Protected attribute: {err}"))
            })?;
            return match value {
                "True" => Ok(true),
                "False" => Ok(false),
                _ => Err(DatabaseError::InvalidFormat(format!(
                    "invalid Protected boolean: {value}"
                ))),
            };
        }
    }
    Ok(false)
}

pub(crate) fn write_preserved_elements(
    writer: &mut quick_xml::Writer<Vec<u8>>,
    elements: &[PreservedXmlElement],
    inner_stream: &mut dyn InnerStreamCipher,
) -> DatabaseResult<()> {
    for element in elements {
        write_preserved_element(writer, element, inner_stream)?;
    }
    Ok(())
}

fn write_preserved_element(
    writer: &mut quick_xml::Writer<Vec<u8>>,
    element: &PreservedXmlElement,
    inner_stream: &mut dyn InnerStreamCipher,
) -> DatabaseResult<()> {
    let start = BytesStart::from_content(element.start.as_str(), element.name_len);
    if element.empty {
        writer
            .write_event(Event::Empty(start))
            .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?;
        return Ok(());
    }

    writer
        .write_event(Event::Start(start))
        .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?;
    for content in &element.content {
        match content {
            PreservedXmlContent::Element(child) => {
                write_preserved_element(writer, child, inner_stream)?
            }
            PreservedXmlContent::Text(text) => writer
                .write_event(Event::Text(BytesText::from_escaped(text.as_str())))
                .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?,
            PreservedXmlContent::CData(text) => writer
                .write_event(Event::CData(BytesCData::new(text.as_str())))
                .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?,
            PreservedXmlContent::Comment(text) => writer
                .write_event(Event::Comment(BytesText::from_escaped(text.as_str())))
                .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?,
            PreservedXmlContent::ProcessingInstruction(text) => writer
                .write_event(Event::PI(BytesPI::new(text.as_str())))
                .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?,
            PreservedXmlContent::Protected(value) => {
                let mut encrypted = value.clone();
                inner_stream.process(&mut encrypted);
                let encoded = base64::engine::general_purpose::STANDARD.encode(encrypted);
                writer
                    .write_event(Event::Text(BytesText::new(&encoded)))
                    .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?;
            }
        }
    }
    let name = &element.start[..element.name_len];
    writer
        .write_event(Event::End(BytesEnd::new(name)))
        .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?;
    Ok(())
}

pub(crate) fn uuid_from_b64(s: &str) -> Option<Uuid> {
    let bytes = b64()
        .decode(s.trim())
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(s.trim()))
        .ok()?;
    if bytes.len() != 16 {
        return None;
    }
    Uuid::from_slice(&bytes).ok()
}

pub(crate) fn required_uuid_from_b64(s: &str, field: &str) -> DatabaseResult<Uuid> {
    uuid_from_b64(s).ok_or_else(|| DatabaseError::InvalidFormat(format!("invalid {field} UUID")))
}

pub(crate) fn uuid_to_b64(uuid: &Uuid) -> String {
    base64::engine::general_purpose::STANDARD.encode(uuid.as_bytes())
}

pub(crate) fn date_from_xml(s: &str) -> DatabaseResult<DateInstant> {
    if s.is_empty() {
        return Ok(DateInstant::Never);
    }
    // Try ISO 8601 / RFC 3339
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(DateInstant::EpochMillis(dt.timestamp_millis()));
    }
    // Try without timezone
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(DateInstant::EpochMillis(dt.and_utc().timestamp_millis()));
    }

    let encoded = base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .or_else(|_| b64().decode(s.trim()))
        .map_err(|e| DatabaseError::InvalidFormat(format!("invalid timestamp: {e}")))?;
    let bytes: [u8; 8] = encoded
        .try_into()
        .map_err(|_| DatabaseError::InvalidFormat("invalid timestamp length".into()))?;
    const KDBX_UNIX_EPOCH_OFFSET_SECONDS: i64 = 62_135_596_800;
    let unix_seconds = i64::from_le_bytes(bytes)
        .checked_sub(KDBX_UNIX_EPOCH_OFFSET_SECONDS)
        .ok_or_else(|| DatabaseError::InvalidFormat("timestamp underflow".into()))?;
    let millis = unix_seconds
        .checked_mul(1000)
        .ok_or_else(|| DatabaseError::InvalidFormat("timestamp overflow".into()))?;
    Ok(DateInstant::EpochMillis(millis))
}

pub(crate) fn date_to_xml(d: &DateInstant) -> String {
    match d {
        DateInstant::EpochMillis(ms) => chrono::DateTime::from_timestamp_millis(*ms)
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

pub(crate) fn icon_id_from_str(s: &str) -> DatabaseResult<u32> {
    s.parse::<u32>()
        .map_err(|e| DatabaseError::InvalidFormat(format!("invalid icon ID: {e}")))
}

pub(crate) fn bool_from_xml(s: &str, field: &str) -> DatabaseResult<bool> {
    match s {
        "True" => Ok(true),
        "False" => Ok(false),
        _ => Err(DatabaseError::InvalidFormat(format!(
            "invalid {field} boolean: {s}"
        ))),
    }
}
