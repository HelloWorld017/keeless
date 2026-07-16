//! KDBX XML reader.

use super::helpers::*;

/// KDBX XML reader.
pub struct KdbxXmlReader;

impl KdbxXmlReader {
    /// Parse a KDBX XML string into a Database.
    /// `inner_stream` is used to decrypt protected field values.
    pub fn read(
        xml: &str,
        inner_stream: &mut dyn InnerStreamCipher,
    ) -> DatabaseResult<Database> {
        let mut reader = Reader::from_str(xml);
        let mut buf = Vec::new();
        let mut db = Database::default();

        loop {
            buf.clear();
            match reader.read_event_into(&mut buf)? {
                Event::Start(ref e) => match tag(e).as_str() {
                    "Meta" => Self::read_meta(&mut reader, &mut db, &mut buf)?,
                    "Root" => Self::read_root(&mut reader, &mut db, inner_stream, &mut buf)?,
                    _ => {}
                },
                Event::End(_) | Event::Eof => break,
                _ => {}
            }
        }

        Ok(db)
    }

    fn read_meta<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        db: &mut Database,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "DatabaseName" => db.name = read_text_content(r, buf)?,
                    "DatabaseDescription" => db.description = read_text_content(r, buf)?,
                    "DefaultUserName" => db.default_username = read_text_content(r, buf)?,
                    "RecycleBinUUID" => {
                        let s = read_text_content(r, buf)?;
                        db.recycle_bin_uuid = uuid_from_b64(&s);
                    }
                    "EntryTemplatesGroup" => {
                        let s = read_text_content(r, buf)?;
                        db.entry_templates_uuid = uuid_from_b64(&s);
                    }
                    "CustomIcons" => Self::read_custom_icons(r, db, buf)?,
                    _ => skip_element(r, e.name().as_ref())?,
                },
                Event::End(e) if tag_end(&e) == "Meta" => return Ok(()),
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }

    fn read_custom_icons<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        db: &mut Database,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) if tag(&e) == "Icon" => {
                    let mut uuid = Uuid::nil();
                    let mut data = Vec::new();
                    let mut name = String::new();
                    loop {
                        buf.clear();
                        match r.read_event_into(buf)? {
                            Event::Start(e2) => match tag(&e2).as_str() {
                                "UUID" => uuid = uuid_from_b64(&read_text_content(r, buf)?).unwrap_or_default(),
                                "Data" => {
                                    let b64_str = read_text_content(r, buf)?;
                                    data = base64::engine::general_purpose::STANDARD
                                        .decode(b64_str.trim())
                                        .unwrap_or_default();
                                }
                                "Name" => name = read_text_content(r, buf)?,
                                _ => skip_element(r, e2.name().as_ref())?,
                            },
                            Event::End(e2) if tag_end(&e2) == "Icon" => break,
                            Event::Eof => break,
                            _ => {}
                        }
                    }
                    db.custom_icons.insert(uuid, IconImageCustom { uuid, data, name, last_modification_time: 0 });
                }
                Event::End(e) if tag_end(&e) == "CustomIcons" => return Ok(()),
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }

    fn read_root<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        db: &mut Database,
        is: &mut dyn InnerStreamCipher,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "Group" => {
                        let root_group = Self::read_group(r, db, is, buf)?;
                        let root_id = root_group.id;
                        // read_group only inserts *child* groups into db.groups,
                        // so the root must be inserted explicitly here.
                        db.groups.insert(root_id, root_group);
                        db.root_group_id = Some(root_id);
                    }
                    "DeletedObjects" => Self::read_deleted_objects(r, db, buf)?,
                    _ => skip_element(r, e.name().as_ref())?,
                },
                Event::End(e) if tag_end(&e) == "Root" => return Ok(()),
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }

    /// Recursively read a `<Group>` element. Inserts all child groups/entries into `db`.
    fn read_group<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        db: &mut Database,
        is: &mut dyn InnerStreamCipher,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<Group> {
        let mut g = Group::new(NodeId::new_uuid());

        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "UUID" => {
                        let s = read_text_content(r, buf)?;
                        if let Some(u) = uuid_from_b64(&s) {
                            g.id = NodeId::from_uuid(u);
                        }
                    }
                    "Name" => g.title = read_text_content(r, buf)?,
                    "Notes" => g.notes = read_text_content(r, buf)?,
                    "IconID" => {
                        let id = read_text_content(r, buf)?;
                        g.icon = IconImage::Standard(IconImageStandard::new(icon_id_from_str(&id)));
                    }
                    "CustomIconUUID" => {
                        let s = read_text_content(r, buf)?;
                        g.custom_icon_uuid = uuid_from_b64(&s);
                    }
                    "IsExpanded" => g.is_expanded = read_text_content(r, buf)? != "False",
                    "EnableSearching" => {
                        let s = read_text_content(r, buf)?;
                        g.enable_searching = s != "False";
                    }
                    "DefaultAutoTypeSequence" => g.default_autotype_sequence = read_text_content(r, buf)?,
                    "Times" => Self::read_group_times(r, &mut g, buf)?,
                    "Group" => {
                        let child = Self::read_group(r, db, is, buf)?;
                        g.add_child_group(child.id);
                        db.groups.insert(child.id, child);
                    }
                    "Entry" => {
                        let entry = Self::read_entry(r, is, buf)?;
                        g.add_child_entry(entry.id);
                        db.entries.insert(entry.id, entry);
                    }
                    _ => skip_element(r, e.name().as_ref())?,
                },
                Event::End(e) if tag_end(&e) == "Group" => break,
                Event::Eof => break,
                _ => {}
            }
        }

        Ok(g)
    }

    fn read_group_times<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        g: &mut Group,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "CreationTime" => g.creation_time = date_from_xml(&read_text_content(r, buf)?),
                    "LastModificationTime" => g.last_modification_time = date_from_xml(&read_text_content(r, buf)?),
                    "ExpiryTime" => g.expiry_time = date_from_xml(&read_text_content(r, buf)?),
                    "Expires" => g.expires = read_text_content(r, buf)? == "True",
                    "UsageCount" => g.usage_count = read_text_content(r, buf)?.parse().unwrap_or(0),
                    _ => skip_element(r, e.name().as_ref())?,
                },
                Event::End(e) if tag_end(&e) == "Times" => return Ok(()),
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }

    fn read_entry<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        is: &mut dyn InnerStreamCipher,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<Entry> {
        let mut e = Entry::new(NodeId::new_uuid());

        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(ref ev) => match tag(ev).as_str() {
                    "UUID" => {
                        let s = read_text_content(r, buf)?;
                        if let Some(u) = uuid_from_b64(&s) {
                            e.id = NodeId::from_uuid(u);
                        }
                    }
                    "IconID" => {
                        let id = read_text_content(r, buf)?;
                        e.icon = IconImage::Standard(IconImageStandard::new(icon_id_from_str(&id)));
                    }
                    "CustomIconUUID" => {
                        let s = read_text_content(r, buf)?;
                        e.custom_icon_uuid = uuid_from_b64(&s);
                    }
                    "ForegroundColor" => e.foreground_color = read_text_content(r, buf)?,
                    "BackgroundColor" => e.background_color = read_text_content(r, buf)?,
                    "OverrideURL" => e.override_url = read_text_content(r, buf)?,
                    "Tags" => {
                        let s = read_text_content(r, buf)?;
                        e.tags = if s.is_empty() { Vec::new() } else { s.split(';').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect() };
                    }
                    "String" => Self::read_entry_string(r, &mut e, is, buf)?,
                    "Binary" => Self::read_entry_binary(r, &mut e, buf)?,
                    "AutoType" => Self::read_auto_type(r, &mut e, buf)?,
                    "Times" => Self::read_entry_times(r, &mut e, buf)?,
                    "History" => Self::read_history(r, &mut e, is, buf)?,
                    _ => skip_element(r, ev.name().as_ref())?,
                },
                Event::End(ev) if tag_end(&ev) == "Entry" => break,
                Event::Eof => break,
                _ => {}
            }
        }

        Ok(e)
    }

    /// Read a `<String>` element (standard or custom field).
    fn read_entry_string<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        entry: &mut Entry,
        is: &mut dyn InnerStreamCipher,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        let mut key = String::new();
        let mut value = String::new();
        let mut protected = false;

        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "Key" => key = read_text_content(r, buf)?,
                    "Value" => {
                        // Check ProtectInMemory attribute
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"ProtectInMemory" {
                                let v = std::str::from_utf8(&attr.value).unwrap_or("");
                                protected = v == "True";
                            }
                        }
                        value = read_text_content(r, buf)?;
                        if protected && !value.is_empty() {
                            // Base64 decode → inner stream decrypt
                            let mut bytes = base64::engine::general_purpose::STANDARD
                                .decode(value.trim())
                                .unwrap_or_default();
                            is.process(&mut bytes);
                            value = String::from_utf8(bytes).unwrap_or_default();
                        }
                    }
                    _ => skip_element(r, e.name().as_ref())?,
                },
                Event::End(e) if tag_end(&e) == "String" => break,
                Event::Eof => break,
                _ => {}
            }
        }

        // Map standard fields
        match key.as_str() {
            "Title" => entry.title = value,
            "UserName" => entry.username = ProtectedString::new_protected(&value),
            "Password" => entry.password = ProtectedString::new_protected(&value),
            "URL" => entry.url = value,
            "Notes" => entry.notes = ProtectedString::new_protected(&value),
            _ => {
                entry.custom_fields.push(EntryField {
                    name: key,
                    value: ProtectedString::new_protected(&value),
                    is_protected: protected,
                });
            }
        }
        Ok(())
    }

    fn read_entry_binary<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        entry: &mut Entry,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        let mut key = String::new();
        let mut value = Vec::new();

        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "Key" => key = read_text_content(r, buf)?,
                    "Value" => {
                        let text = read_text_content(r, buf)?;
                        // KDBX 3.1: base64-encoded inline binary data
                        value = base64::engine::general_purpose::STANDARD
                            .decode(text.trim())
                            .unwrap_or_default();
                    }
                    _ => skip_element(r, e.name().as_ref())?,
                },
                Event::End(e) if tag_end(&e) == "Binary" => break,
                Event::Eof => break,
                _ => {}
            }
        }

        if !key.is_empty() {
            entry.binaries.push((key, value));
        }
        Ok(())
    }

    fn read_auto_type<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        entry: &mut Entry,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "Enabled" => entry.autotype_enabled = read_text_content(r, buf)? != "False",
                    "DefaultSequence" => entry.autotype_sequence = read_text_content(r, buf)?,
                    _ => skip_element(r, e.name().as_ref())?,
                },
                Event::End(e) if tag_end(&e) == "AutoType" => return Ok(()),
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }

    fn read_entry_times<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        entry: &mut Entry,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "CreationTime" => entry.creation_time = date_from_xml(&read_text_content(r, buf)?),
                    "LastModificationTime" => entry.last_modification_time = date_from_xml(&read_text_content(r, buf)?),
                    "ExpiryTime" => entry.expiry_time = date_from_xml(&read_text_content(r, buf)?),
                    "Expires" => entry.expires = read_text_content(r, buf)? == "True",
                    "UsageCount" => entry.usage_count = read_text_content(r, buf)?.parse().unwrap_or(0),
                    _ => skip_element(r, e.name().as_ref())?,
                },
                Event::End(e) if tag_end(&e) == "Times" => return Ok(()),
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }

    fn read_history<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        entry: &mut Entry,
        is: &mut dyn InnerStreamCipher,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) if tag(&e) == "Entry" => {
                    let h = Self::read_entry(r, is, buf)?;
                    entry.history.push(h);
                }
                Event::End(e) if tag_end(&e) == "History" => return Ok(()),
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }

    fn read_deleted_objects<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        db: &mut Database,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) if tag(&e) == "DeletedObject" => {
                    let mut id = NodeId::new_uuid();
                    loop {
                        buf.clear();
                        match r.read_event_into(buf)? {
                            Event::Start(e2) => match tag(&e2).as_str() {
                                "UUID" => {
                                    let s = read_text_content(r, buf)?;
                                    if let Some(u) = uuid_from_b64(&s) {
                                        id = NodeId::from_uuid(u);
                                    }
                                }
                                _ => skip_element(r, e2.name().as_ref())?,
                            },
                            Event::End(e2) if tag_end(&e2) == "DeletedObject" => break,
                            Event::Eof => break,
                            _ => {}
                        }
                    }
                    db.deleted_objects.push(id);
                }
                Event::End(e) if tag_end(&e) == "DeletedObjects" => return Ok(()),
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }
}
