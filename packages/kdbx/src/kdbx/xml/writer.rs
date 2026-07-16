//! KDBX XML writer.

use super::helpers::*;

/// KDBX XML writer.
pub struct KdbxXmlWriter;

impl KdbxXmlWriter {
    /// Serialize a Database to XML string.
    /// `inner_stream` encrypts protected field values.
    pub fn write(
        db: &Database,
        inner_stream: &mut dyn InnerStreamCipher,
    ) -> DatabaseResult<String> {
        let mut w = quick_xml::Writer::new(Vec::new());

        w.write_event(Event::Decl(BytesDecl::new("1.0", Some("utf-8"), None)))
            .map_err(|e| DatabaseError::InvalidFormat(e.to_string()))?;

        Self::write_element(&mut w, "KeePassFile", |w| {
            Self::write_meta(w, db)?;
            Self::write_element(w, "Root", |w| {
                if let Some(rg) = db.root_group() {
                    Self::write_group(w, rg, db, inner_stream)?;
                }
                // DeletedObjects
                if !db.deleted_objects.is_empty() {
                    Self::write_element(w, "DeletedObjects", |w| {
                        for id in &db.deleted_objects {
                            Self::write_element(w, "DeletedObject", |w| {
                                if let Some(u) = id.as_uuid() {
                                    Self::write_tag(w, "UUID", &uuid_to_b64(u))?;
                                }
                                Ok(())
                            })?;
                        }
                        Ok(())
                    })?;
                }
                Ok(())
            })?;
            Ok(())
        })?;

        let bytes = w.into_inner();
        String::from_utf8(bytes).map_err(|e| DatabaseError::InvalidFormat(e.to_string()))
    }

    fn write_meta(w: &mut quick_xml::Writer<Vec<u8>>, db: &Database) -> DatabaseResult<()> {
        Self::write_element(w, "Meta", |w| {
            Self::write_tag(w, "Generator", "KeePassFR Rust")?;
            Self::write_tag(w, "DatabaseName", &db.name)?;
            Self::write_tag(w, "DatabaseDescription", &db.description)?;
            Self::write_tag(w, "DefaultUserName", &db.default_username)?;
            if let Some(u) = db.recycle_bin_uuid {
                Self::write_tag(w, "RecycleBinUUID", &uuid_to_b64(&u))?;
            }
            if let Some(u) = db.entry_templates_uuid {
                Self::write_tag(w, "EntryTemplatesGroup", &uuid_to_b64(&u))?;
            }
            // MemoryProtection defaults
            Self::write_element(w, "MemoryProtection", |w| {
                Self::write_tag(w, "ProtectTitle", "False")?;
                Self::write_tag(w, "ProtectUserName", "False")?;
                Self::write_tag(w, "ProtectPassword", "True")?;
                Self::write_tag(w, "ProtectURL", "False")?;
                Self::write_tag(w, "ProtectNotes", "False")?;
                Ok(())
            })?;
            // CustomIcons
            if !db.custom_icons.is_empty() {
                Self::write_element(w, "CustomIcons", |w| {
                    for ci in db.custom_icons.values() {
                        Self::write_element(w, "Icon", |w| {
                            Self::write_tag(w, "UUID", &uuid_to_b64(&ci.uuid))?;
                            Self::write_tag(w, "Data", &base64::engine::general_purpose::STANDARD.encode(&ci.data))?;
                            if !ci.name.is_empty() {
                                Self::write_tag(w, "Name", &ci.name)?;
                            }
                            Ok(())
                        })?;
                    }
                    Ok(())
                })?;
            }
            Ok(())
        })
    }

    fn write_group(
        w: &mut quick_xml::Writer<Vec<u8>>,
        g: &Group,
        db: &Database,
        is: &mut dyn InnerStreamCipher,
    ) -> DatabaseResult<()> {
        Self::write_element(w, "Group", |w| {
            if let Some(u) = g.id.as_uuid() {
                Self::write_tag(w, "UUID", &uuid_to_b64(u))?;
            }
            Self::write_tag(w, "Name", &g.title)?;
            if !g.notes.is_empty() {
                Self::write_tag(w, "Notes", &g.notes)?;
            }
            Self::write_icon_id(w, &g.icon)?;
            if let Some(u) = g.custom_icon_uuid {
                Self::write_tag(w, "CustomIconUUID", &uuid_to_b64(&u))?;
            }
            Self::write_times(w, g.creation_time, g.last_modification_time, g.expiry_time, g.expires, g.usage_count)?;
            Self::write_tag(w, "IsExpanded", if g.is_expanded { "True" } else { "False" })?;
            Self::write_tag(w, "DefaultAutoTypeSequence", &g.default_autotype_sequence)?;
            // Child groups
            for cid in &g.child_group_ids {
                if let Some(child) = db.groups.get(cid) {
                    Self::write_group(w, child, db, is)?;
                }
            }
            // Child entries
            for eid in &g.child_entry_ids {
                if let Some(entry) = db.entries.get(eid) {
                    Self::write_entry(w, entry, is)?;
                }
            }
            Ok(())
        })
    }

    fn write_entry(
        w: &mut quick_xml::Writer<Vec<u8>>,
        e: &Entry,
        is: &mut dyn InnerStreamCipher,
    ) -> DatabaseResult<()> {
        Self::write_element(w, "Entry", |w| {
            if let Some(u) = e.id.as_uuid() {
                Self::write_tag(w, "UUID", &uuid_to_b64(u))?;
            }
            Self::write_icon_id(w, &e.icon)?;
            if let Some(u) = e.custom_icon_uuid {
                Self::write_tag(w, "CustomIconUUID", &uuid_to_b64(&u))?;
            }
            if !e.foreground_color.is_empty() {
                Self::write_tag(w, "ForegroundColor", &e.foreground_color)?;
            }
            if !e.background_color.is_empty() {
                Self::write_tag(w, "BackgroundColor", &e.background_color)?;
            }
            if !e.override_url.is_empty() {
                Self::write_tag(w, "OverrideURL", &e.override_url)?;
            }
            if !e.tags.is_empty() {
                Self::write_tag(w, "Tags", &e.tags.join(";"))?;
            }
            // Standard fields
            Self::write_field(w, "Title", &e.title, false, is)?;
            Self::write_field(w, "UserName", e.username.as_str(), false, is)?;
            Self::write_field(w, "Password", e.password.as_str(), true, is)?;
            Self::write_field(w, "URL", &e.url, false, is)?;
            Self::write_field(w, "Notes", e.notes.as_str(), false, is)?;
            // Custom fields
            for cf in &e.custom_fields {
                Self::write_field(w, &cf.name, cf.value.as_str(), cf.is_protected, is)?;
            }
            // AutoType
            Self::write_element(w, "AutoType", |w| {
                Self::write_tag(w, "Enabled", if e.autotype_enabled { "True" } else { "False" })?;
                Self::write_tag(w, "DefaultSequence", &e.autotype_sequence)?;
                Ok(())
            })?;
            // Binary attachments
            for (name, data) in &e.binaries {
                Self::write_element(w, "Binary", |w| {
                    Self::write_tag(w, "Key", name)?;
                    Self::write_tag(w, "Value", &base64::engine::general_purpose::STANDARD.encode(data))?;
                    Ok(())
                })?;
            }
            // Times
            Self::write_times(w, e.creation_time, e.last_modification_time, e.expiry_time, e.expires, e.usage_count)?;
            // History
            if !e.history.is_empty() {
                Self::write_element(w, "History", |w| {
                    for h in &e.history {
                        Self::write_entry(w, h, is)?;
                    }
                    Ok(())
                })?;
            }
            Ok(())
        })
    }

    fn write_field(
        w: &mut quick_xml::Writer<Vec<u8>>,
        key: &str,
        value: &str,
        protect: bool,
        is: &mut dyn InnerStreamCipher,
    ) -> DatabaseResult<()> {
        Self::write_element(w, "String", |w| {
            Self::write_tag(w, "Key", key)?;
            if protect && !value.is_empty() {
                // Encrypt with inner stream, then base64 encode
                let mut bytes = value.as_bytes().to_vec();
                is.process(&mut bytes);
                let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let mut val_elem = BytesStart::new("Value");
                val_elem.push_attribute(("ProtectInMemory", "True"));
                w.write_event(Event::Start(val_elem))
                    .map_err(|e| DatabaseError::InvalidFormat(e.to_string()))?;
                w.write_event(Event::Text(BytesText::new(&encoded)))
                    .map_err(|e| DatabaseError::InvalidFormat(e.to_string()))?;
                w.write_event(Event::End(BytesEnd::new("Value")))
                    .map_err(|e| DatabaseError::InvalidFormat(e.to_string()))?;
            } else {
                Self::write_tag(w, "Value", value)?;
            }
            Ok(())
        })
    }

    fn write_times(
        w: &mut quick_xml::Writer<Vec<u8>>,
        created: DateInstant,
        modified: DateInstant,
        expiry: DateInstant,
        expires: bool,
        usage_count: i64,
    ) -> DatabaseResult<()> {
        Self::write_element(w, "Times", |w| {
            Self::write_tag(w, "CreationTime", &date_to_xml(&created))?;
            Self::write_tag(w, "LastModificationTime", &date_to_xml(&modified))?;
            Self::write_tag(w, "LastAccessTime", &date_to_xml(&modified))?; // Use modified as access
            Self::write_tag(w, "ExpiryTime", &date_to_xml(&expiry))?;
            Self::write_tag(w, "Expires", if expires { "True" } else { "False" })?;
            Self::write_tag(w, "UsageCount", &usage_count.to_string())?;
            Ok(())
        })
    }

    fn write_icon_id(w: &mut quick_xml::Writer<Vec<u8>>, icon: &IconImage) -> DatabaseResult<()> {
        match icon {
            IconImage::Standard(s) => Self::write_tag(w, "IconID", &s.icon_id.to_string()),
            IconImage::Custom(_) => Self::write_tag(w, "IconID", "0"),
        }
    }

    // ─── low-level helpers ──────────────────────────

    fn write_element<F>(w: &mut quick_xml::Writer<Vec<u8>>, name: &str, content: F) -> DatabaseResult<()>
    where
        F: FnOnce(&mut quick_xml::Writer<Vec<u8>>) -> DatabaseResult<()>,
    {
        w.write_event(Event::Start(BytesStart::new(name)))
            .map_err(|e| DatabaseError::InvalidFormat(e.to_string()))?;
        content(w)?;
        w.write_event(Event::End(BytesEnd::new(name)))
            .map_err(|e| DatabaseError::InvalidFormat(e.to_string()))?;
        Ok(())
    }

    fn write_tag(w: &mut quick_xml::Writer<Vec<u8>>, name: &str, value: &str) -> DatabaseResult<()> {
        w.write_event(Event::Start(BytesStart::new(name)))
            .map_err(|e| DatabaseError::InvalidFormat(e.to_string()))?;
        if !value.is_empty() {
            w.write_event(Event::Text(BytesText::new(value)))
                .map_err(|e| DatabaseError::InvalidFormat(e.to_string()))?;
        }
        w.write_event(Event::End(BytesEnd::new(name)))
            .map_err(|e| DatabaseError::InvalidFormat(e.to_string()))?;
        Ok(())
    }
}
