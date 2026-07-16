//! Shared helpers for XML reading/writing.

pub(crate) use base64::Engine;
pub(crate) use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
pub(crate) use quick_xml::Reader;
pub(crate) use uuid::Uuid;

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
