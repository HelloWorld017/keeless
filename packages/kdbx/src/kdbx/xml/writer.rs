//! KDBX XML writer.

mod data;
mod entry;
mod group;
mod meta;

use super::helpers::*;
use crate::crypto::memory_protection::MemoryUnlockSession;
use crate::model::db::composite_key::CompositeKey;
use crate::model::db::database::DatabaseVersion;
use crate::model::entry::{memory_field, EntryFieldId};

type XmlWriter = quick_xml::Writer<Vec<u8>>;

/// KDBX XML writer.
pub struct KdbxXmlWriter;

impl KdbxXmlWriter {
    /// Serialize a Database to XML string.
    /// `inner_stream` encrypts protected field values.
    pub fn write(
        db: &Database,
        inner_stream: &mut dyn InnerStreamCipher,
    ) -> DatabaseResult<String> {
        let mut memory = MemoryWriteAccess::Unsealed;
        Self::write_internal(db, inner_stream, &mut memory, None)
    }

    pub(crate) fn write_with_credentials(
        db: &Database,
        inner_stream: &mut dyn InnerStreamCipher,
        composite_key: &CompositeKey,
    ) -> DatabaseResult<String> {
        let mut memory = MemoryWriteAccess::Unlocked(db.memory_unlock(composite_key));
        Self::write_internal(db, inner_stream, &mut memory, None)
    }

    pub(crate) fn write_with_credentials_and_header_hash(
        db: &Database,
        inner_stream: &mut dyn InnerStreamCipher,
        composite_key: &CompositeKey,
        header_hash: &[u8],
    ) -> DatabaseResult<String> {
        let mut memory = MemoryWriteAccess::Unlocked(db.memory_unlock(composite_key));
        Self::write_internal(db, inner_stream, &mut memory, Some(header_hash))
    }

    fn write_internal(
        db: &Database,
        inner_stream: &mut dyn InnerStreamCipher,
        memory: &mut MemoryWriteAccess<'_>,
        header_hash: Option<&[u8]>,
    ) -> DatabaseResult<String> {
        let mut writer = XmlWriter::new(Vec::new());
        let mut binary_index = 0usize;
        let use_binary_refs = matches!(db.version, DatabaseVersion::KDBX4);

        writer
            .write_event(Event::Decl(BytesDecl::new("1.0", Some("utf-8"), None)))
            .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?;

        write_element(&mut writer, "KeePassFile", |writer| {
            meta::write_meta(writer, db, inner_stream, header_hash)?;
            write_element(writer, "Root", |writer| {
                if let Some(root_group) = db.root_group() {
                    group::write_group(
                        writer,
                        root_group,
                        db,
                        inner_stream,
                        use_binary_refs,
                        &mut binary_index,
                        memory,
                    )?;
                }
                data::write_deleted_objects(writer, db, inner_stream)?;
                write_preserved_elements(writer, &db.xml_extensions.root, inner_stream)
            })?;
            write_preserved_elements(writer, &db.xml_extensions.keepass_file, inner_stream)
        })?;

        String::from_utf8(writer.into_inner())
            .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))
    }
}

pub(super) enum MemoryWriteAccess<'a> {
    Unsealed,
    Unlocked(MemoryUnlockSession<'a>),
}

impl MemoryWriteAccess<'_> {
    pub(super) fn with_entry_field<T>(
        &mut self,
        entry: &Entry,
        id: EntryFieldId,
        use_value: impl FnOnce(&str) -> DatabaseResult<T>,
    ) -> DatabaseResult<T> {
        let field = entry
            .field(id)
            .ok_or_else(|| DatabaseError::InvalidFormat(format!("unknown field ID: {id}")))?;
        let memory_field = memory_field(id, field.name());
        match self {
            Self::Unlocked(unlock) => {
                field
                    .value
                    .with_plaintext(unlock, entry.id, &memory_field, use_value)
            }
            Self::Unsealed if field.value.is_memory_protected() => {
                Err(DatabaseError::InvalidCredentials)
            }
            Self::Unsealed => use_value(field.value.as_str()),
        }
    }
}

pub(super) fn write_element<F>(writer: &mut XmlWriter, name: &str, content: F) -> DatabaseResult<()>
where
    F: FnOnce(&mut XmlWriter) -> DatabaseResult<()>,
{
    writer
        .write_event(Event::Start(BytesStart::new(name)))
        .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?;
    content(writer)?;
    writer
        .write_event(Event::End(BytesEnd::new(name)))
        .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?;
    Ok(())
}

pub(super) fn write_tag(writer: &mut XmlWriter, name: &str, value: &str) -> DatabaseResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new(name)))
        .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?;
    if !value.is_empty() {
        writer
            .write_event(Event::Text(BytesText::new(value)))
            .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?;
    }
    writer
        .write_event(Event::End(BytesEnd::new(name)))
        .map_err(|err| DatabaseError::InvalidFormat(err.to_string()))?;
    Ok(())
}

pub(super) fn write_icon_id(writer: &mut XmlWriter, icon: &IconImage) -> DatabaseResult<()> {
    match icon {
        IconImage::Standard(standard) => write_tag(writer, "IconID", &standard.icon_id.to_string()),
        IconImage::Custom(_) => write_tag(writer, "IconID", "0"),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn write_times(
    writer: &mut XmlWriter,
    created: DateInstant,
    modified: DateInstant,
    accessed: DateInstant,
    expiry: DateInstant,
    expires: bool,
    usage_count: i64,
    location_changed: DateInstant,
    extensions: &[PreservedXmlElement],
    inner_stream: &mut dyn InnerStreamCipher,
) -> DatabaseResult<()> {
    write_element(writer, "Times", |writer| {
        write_tag(writer, "CreationTime", &date_to_xml(&created))?;
        write_tag(writer, "LastModificationTime", &date_to_xml(&modified))?;
        write_tag(writer, "LastAccessTime", &date_to_xml(&accessed))?;
        write_tag(writer, "ExpiryTime", &date_to_xml(&expiry))?;
        write_tag(writer, "Expires", bool_xml(expires))?;
        write_tag(writer, "UsageCount", &usage_count.to_string())?;
        write_tag(writer, "LocationChanged", &date_to_xml(&location_changed))?;
        write_preserved_elements(writer, extensions, inner_stream)
    })
}

pub(super) fn bool_xml(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}
