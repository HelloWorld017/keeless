//! Shared helpers for XML reading/writing.

pub(crate) use base64::Engine;
pub(crate) use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
pub(crate) use quick_xml::Reader;
pub(crate) use uuid::Uuid;

pub(crate) use crate::crypto::inner_stream::InnerStreamCipher;
pub(crate) use crate::model::core::date::DateInstant;
pub(crate) use crate::model::entry::{Entry, EntryField};
pub(crate) use crate::model::group::Group;
pub(crate) use crate::model::meta::icon::{IconImage, IconImageCustom, IconImageStandard};
pub(crate) use crate::model::core::node::NodeId;
pub(crate) use crate::model::core::security::ProtectedString;
pub(crate) use crate::model::exception::{DatabaseError, DatabaseResult};
pub(crate) use crate::model::db::database::Database;

pub(crate) fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD_NO_PAD
}

pub(crate) fn tag(e: &quick_xml::events::BytesStart<'_>) -> String {
    std::str::from_utf8(e.name().as_ref()).unwrap_or("").to_string()
}

pub(crate) fn tag_end(e: &quick_xml::events::BytesEnd<'_>) -> String {
    std::str::from_utf8(e.name().as_ref()).unwrap_or("").to_string()
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
                return Ok(t.unescape().map_err(|e| DatabaseError::InvalidFormat(e.to_string()))?.into_owned());
            }
            Event::End(_) => return Ok(String::new()),
            Event::Eof => return Ok(String::new()),
            _ => {}
        }
    }
}

/// Skip to matching end tag.
pub(crate) fn skip_element<R: std::io::BufRead>(
    reader: &mut quick_xml::Reader<R>,
    end_tag: &[u8],
) -> DatabaseResult<()> {
    let mut depth: u32 = 1;
    let mut local_buf = Vec::new();
    loop {
        local_buf.clear();
        match reader.read_event_into(&mut local_buf)? {
            Event::Start(e) if e.name().as_ref() == end_tag => depth += 1,
            Event::End(e) if e.name().as_ref() == end_tag => {
                depth -= 1;
                if depth == 0 {
                    return Ok(());
                }
            }
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

pub(crate) fn uuid_from_b64(s: &str) -> Option<Uuid> {
    let bytes = b64().decode(s.trim()).ok()?;
    if bytes.len() != 16 {
        // Try with padding
        let bytes2 = base64::engine::general_purpose::STANDARD.decode(s.trim()).ok()?;
        if bytes2.len() != 16 {
            return None;
        }
        return Uuid::from_slice(&bytes2).ok();
    }
    Uuid::from_slice(&bytes).ok()
}

pub(crate) fn uuid_to_b64(uuid: &Uuid) -> String {
    b64().encode(uuid.as_bytes())
}

pub(crate) fn date_from_xml(s: &str) -> DateInstant {
    if s.is_empty() {
        return DateInstant::Never;
    }
    // Try ISO 8601 / RFC 3339
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return DateInstant::EpochMillis(dt.timestamp_millis());
    }
    // Try without timezone
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return DateInstant::EpochMillis(dt.and_utc().timestamp_millis());
    }
    DateInstant::Never
}

pub(crate) fn date_to_xml(d: &DateInstant) -> String {
    match d {
        DateInstant::EpochMillis(ms) => {
            let secs = ms / 1000;
            let nsecs = ((ms % 1000).max(0) as u32) * 1_000_000;
            chrono::DateTime::from_timestamp(secs, nsecs)
                .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .unwrap_or_default()
        }
        _ => String::new(),
    }
}

pub(crate) fn icon_id_from_str(s: &str) -> u32 {
    s.parse::<u32>().unwrap_or(0)
}
