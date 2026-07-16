//! KDBX XML reader.

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
        Self::validate_nesting(xml)?;
        let mut reader = Reader::from_str(xml);
        let mut buf = Vec::new();
        let mut db = Database::default();

        loop {
            buf.clear();
            match reader.read_event_into(&mut buf)? {
                Event::Start(ref e) => match tag(e).as_str() {
                    "Meta" => Self::read_meta(&mut reader, &mut db, &mut buf)?,
                    "Root" => {
                        Self::read_root(&mut reader, &mut db, inner_stream, binaries, &mut buf)?
                    }
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
                    "Generator" => {
                        let _ = read_text_content(r, buf)?;
                    }
                    "DatabaseDescription" => db.description = read_text_content(r, buf)?,
                    "DefaultUserName" => db.default_username = read_text_content(r, buf)?,
                    "RecycleBinUUID" => {
                        let s = read_text_content(r, buf)?;
                        db.recycle_bin_uuid = if s.is_empty() {
                            None
                        } else {
                            Some(required_uuid_from_b64(&s, "recycle bin")?)
                        };
                    }
                    "EntryTemplatesGroup" => {
                        let s = read_text_content(r, buf)?;
                        db.entry_templates_uuid = if s.is_empty() {
                            None
                        } else {
                            Some(required_uuid_from_b64(&s, "entry templates group")?)
                        };
                    }
                    "CustomIcons" => Self::read_custom_icons(r, db, buf)?,
                    "MemoryProtection" => Self::read_memory_protection(r, db, buf)?,
                    "CustomData" => Self::read_custom_data(
                        r,
                        &mut db.custom_data,
                        &mut db.contains_unsupported_xml,
                        buf,
                    )?,
                    _ => {
                        db.contains_unsupported_xml = true;
                        skip_element(r, e.name().as_ref())?;
                    }
                },
                Event::End(e) if tag_end(&e) == "Meta" => return Ok(()),
                Event::Empty(e) => match tag(&e).as_str() {
                    "Generator"
                    | "DatabaseName"
                    | "DatabaseDescription"
                    | "DefaultUserName"
                    | "RecycleBinUUID"
                    | "EntryTemplatesGroup"
                    | "CustomIcons"
                    | "CustomData" => {}
                    _ => db.contains_unsupported_xml = true,
                },
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }

    fn read_memory_protection<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        db: &mut Database,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => {
                    let name = tag(&e);
                    let value = read_text_content(r, buf)? == "True";
                    match name.as_str() {
                        "ProtectTitle" => db.memory_protection.protect_title = value,
                        "ProtectUserName" => db.memory_protection.protect_username = value,
                        "ProtectPassword" => db.memory_protection.protect_password = value,
                        "ProtectURL" => db.memory_protection.protect_url = value,
                        "ProtectNotes" => db.memory_protection.protect_notes = value,
                        "AutoEnableVisualHiding" => {
                            db.memory_protection.auto_enable_visual_hiding = value
                        }
                        _ => db.contains_unsupported_xml = true,
                    }
                }
                Event::End(e) if tag_end(&e) == "MemoryProtection" => return Ok(()),
                Event::Empty(e) => match tag(&e).as_str() {
                    "ProtectTitle"
                    | "ProtectUserName"
                    | "ProtectPassword"
                    | "ProtectURL"
                    | "ProtectNotes"
                    | "AutoEnableVisualHiding" => {}
                    _ => db.contains_unsupported_xml = true,
                },
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
                                "UUID" => {
                                    uuid = required_uuid_from_b64(
                                        &read_text_content(r, buf)?,
                                        "custom icon",
                                    )?
                                }
                                "Data" => {
                                    let b64_str = read_text_content(r, buf)?;
                                    data = base64::engine::general_purpose::STANDARD
                                        .decode(b64_str.trim())
                                        .map_err(|err| {
                                            DatabaseError::InvalidFormat(format!(
                                                "invalid custom icon data: {err}"
                                            ))
                                        })?;
                                }
                                "Name" => name = read_text_content(r, buf)?,
                                _ => {
                                    db.contains_unsupported_xml = true;
                                    skip_element(r, e2.name().as_ref())?;
                                }
                            },
                            Event::End(e2) if tag_end(&e2) == "Icon" => break,
                            Event::Empty(e2) => match tag(&e2).as_str() {
                                "UUID" | "Data" | "Name" => {}
                                _ => db.contains_unsupported_xml = true,
                            },
                            Event::Eof => break,
                            _ => {}
                        }
                    }
                    db.custom_icons.insert(
                        uuid,
                        IconImageCustom {
                            uuid,
                            data,
                            name,
                            last_modification_time: 0,
                        },
                    );
                }
                Event::End(e) if tag_end(&e) == "CustomIcons" => return Ok(()),
                Event::Empty(_) => db.contains_unsupported_xml = true,
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }

    fn read_root<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        db: &mut Database,
        is: &mut dyn InnerStreamCipher,
        binaries: &[(Vec<u8>, bool)],
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "Group" => {
                        let root_group = Self::read_group(r, db, is, binaries, buf)?;
                        let root_id = root_group.id;
                        // read_group only inserts *child* groups into db.groups,
                        // so the root must be inserted explicitly here.
                        db.groups.insert(root_id, root_group);
                        db.root_group_id = Some(root_id);
                    }
                    "DeletedObjects" => Self::read_deleted_objects(r, db, buf)?,
                    _ => {
                        db.contains_unsupported_xml = true;
                        skip_element(r, e.name().as_ref())?;
                    }
                },
                Event::End(e) if tag_end(&e) == "Root" => return Ok(()),
                Event::Empty(e) if tag(&e) != "DeletedObjects" => {
                    db.contains_unsupported_xml = true
                }
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
        binaries: &[(Vec<u8>, bool)],
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<Group> {
        let mut g = Group::new(NodeId::new_uuid());

        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "UUID" => {
                        let s = read_text_content(r, buf)?;
                        g.id = NodeId::from_uuid(required_uuid_from_b64(&s, "group")?);
                    }
                    "Name" => g.title = read_text_content(r, buf)?,
                    "Notes" => g.notes = read_text_content(r, buf)?,
                    "IconID" => {
                        let id = read_text_content(r, buf)?;
                        g.icon =
                            IconImage::Standard(IconImageStandard::new(icon_id_from_str(&id)?));
                    }
                    "CustomIconUUID" => {
                        let s = read_text_content(r, buf)?;
                        g.custom_icon_uuid = if s.is_empty() {
                            None
                        } else {
                            Some(required_uuid_from_b64(&s, "group custom icon")?)
                        };
                    }
                    "IsExpanded" => g.is_expanded = read_text_content(r, buf)? != "False",
                    "EnableSearching" => {
                        let s = read_text_content(r, buf)?;
                        g.enable_searching = s != "False";
                    }
                    "DefaultAutoTypeSequence" => {
                        g.default_autotype_sequence = read_text_content(r, buf)?
                    }
                    "Times" => Self::read_group_times(r, &mut g, db, buf)?,
                    "Group" => {
                        let child = Self::read_group(r, db, is, binaries, buf)?;
                        g.add_child_group(child.id);
                        db.groups.insert(child.id, child);
                    }
                    "Entry" => {
                        let entry = Self::read_entry(r, db, is, binaries, buf)?;
                        g.add_child_entry(entry.id);
                        db.entries.insert(entry.id, entry);
                    }
                    "CustomData" => Self::read_custom_data(
                        r,
                        &mut g.custom_data,
                        &mut db.contains_unsupported_xml,
                        buf,
                    )?,
                    _ => {
                        db.contains_unsupported_xml = true;
                        skip_element(r, e.name().as_ref())?;
                    }
                },
                Event::End(e) if tag_end(&e) == "Group" => break,
                Event::Empty(e) => match tag(&e).as_str() {
                    "UUID"
                    | "Name"
                    | "Notes"
                    | "CustomIconUUID"
                    | "DefaultAutoTypeSequence"
                    | "CustomData" => {}
                    _ => db.contains_unsupported_xml = true,
                },
                Event::Eof => break,
                _ => {}
            }
        }

        Ok(g)
    }

    fn read_group_times<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        g: &mut Group,
        db: &mut Database,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "CreationTime" => g.creation_time = date_from_xml(&read_text_content(r, buf)?)?,
                    "LastModificationTime" => {
                        g.last_modification_time = date_from_xml(&read_text_content(r, buf)?)?
                    }
                    "LastAccessTime" => {
                        g.last_access_time = date_from_xml(&read_text_content(r, buf)?)?
                    }
                    "LocationChanged" => {
                        g.location_changed = date_from_xml(&read_text_content(r, buf)?)?
                    }
                    "ExpiryTime" => g.expiry_time = date_from_xml(&read_text_content(r, buf)?)?,
                    "Expires" => g.expires = read_text_content(r, buf)? == "True",
                    "UsageCount" => {
                        g.usage_count = read_text_content(r, buf)?.parse().map_err(|err| {
                            DatabaseError::InvalidFormat(format!(
                                "invalid group usage count: {err}"
                            ))
                        })?
                    }
                    _ => {
                        db.contains_unsupported_xml = true;
                        skip_element(r, e.name().as_ref())?;
                    }
                },
                Event::End(e) if tag_end(&e) == "Times" => return Ok(()),
                Event::Empty(e) => match tag(&e).as_str() {
                    "CreationTime"
                    | "LastModificationTime"
                    | "LastAccessTime"
                    | "LocationChanged"
                    | "ExpiryTime" => {}
                    _ => db.contains_unsupported_xml = true,
                },
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }

    fn read_entry<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        db: &mut Database,
        is: &mut dyn InnerStreamCipher,
        binaries: &[(Vec<u8>, bool)],
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<Entry> {
        let mut e = Entry::new(NodeId::new_uuid());

        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(ref ev) => match tag(ev).as_str() {
                    "UUID" => {
                        let s = read_text_content(r, buf)?;
                        e.id = NodeId::from_uuid(required_uuid_from_b64(&s, "entry")?);
                    }
                    "IconID" => {
                        let id = read_text_content(r, buf)?;
                        e.icon =
                            IconImage::Standard(IconImageStandard::new(icon_id_from_str(&id)?));
                    }
                    "CustomIconUUID" => {
                        let s = read_text_content(r, buf)?;
                        e.custom_icon_uuid = if s.is_empty() {
                            None
                        } else {
                            Some(required_uuid_from_b64(&s, "entry custom icon")?)
                        };
                    }
                    "ForegroundColor" => e.foreground_color = read_text_content(r, buf)?,
                    "BackgroundColor" => e.background_color = read_text_content(r, buf)?,
                    "OverrideURL" => e.override_url = read_text_content(r, buf)?,
                    "Tags" => {
                        let s = read_text_content(r, buf)?;
                        e.tags = if s.is_empty() {
                            Vec::new()
                        } else {
                            s.split(';')
                                .map(|t| t.trim().to_string())
                                .filter(|t| !t.is_empty())
                                .collect()
                        };
                    }
                    "String" => Self::read_entry_string(
                        r,
                        &mut e,
                        is,
                        &mut db.contains_unsupported_xml,
                        buf,
                    )?,
                    "Binary" => Self::read_entry_binary(
                        r,
                        &mut e,
                        binaries,
                        &mut db.contains_unsupported_xml,
                        buf,
                    )?,
                    "AutoType" => {
                        Self::read_auto_type(r, &mut e, &mut db.contains_unsupported_xml, buf)?
                    }
                    "Times" => Self::read_entry_times(r, &mut e, db, buf)?,
                    "History" => Self::read_history(r, &mut e, db, is, binaries, buf)?,
                    "CustomData" => Self::read_custom_data(
                        r,
                        &mut e.custom_data,
                        &mut db.contains_unsupported_xml,
                        buf,
                    )?,
                    _ => {
                        db.contains_unsupported_xml = true;
                        skip_element(r, ev.name().as_ref())?;
                    }
                },
                Event::End(ev) if tag_end(&ev) == "Entry" => break,
                Event::Empty(ev) => match tag(&ev).as_str() {
                    "UUID" | "CustomIconUUID" | "ForegroundColor" | "BackgroundColor"
                    | "OverrideURL" | "Tags" | "History" | "CustomData" => {}
                    _ => db.contains_unsupported_xml = true,
                },
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
        unsupported: &mut bool,
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
                        for attr in e.attributes() {
                            let attr = attr?;
                            if attr.key.as_ref() == b"Protected" {
                                let v = std::str::from_utf8(&attr.value).map_err(|err| {
                                    DatabaseError::InvalidFormat(format!(
                                        "invalid Protected attribute: {err}"
                                    ))
                                })?;
                                protected = v == "True";
                            }
                        }
                        value = read_text_content(r, buf)?;
                        if protected && !value.is_empty() {
                            // Base64 decode → inner stream decrypt
                            let mut bytes = base64::engine::general_purpose::STANDARD
                                .decode(value.trim())
                                .map_err(|err| {
                                    DatabaseError::InvalidFormat(format!(
                                        "invalid protected value base64: {err}"
                                    ))
                                })?;
                            is.process(&mut bytes);
                            value = String::from_utf8(bytes).map_err(|err| {
                                DatabaseError::InvalidFormat(format!(
                                    "protected value is not UTF-8: {err}"
                                ))
                            })?;
                        }
                    }
                    _ => {
                        *unsupported = true;
                        skip_element(r, e.name().as_ref())?;
                    }
                },
                Event::End(e) if tag_end(&e) == "String" => break,
                Event::Empty(e) if tag(&e) == "Value" => {
                    for attr in e.attributes() {
                        let attr = attr?;
                        if attr.key.as_ref() == b"Protected" {
                            let value = std::str::from_utf8(&attr.value).map_err(|err| {
                                DatabaseError::InvalidFormat(format!(
                                    "invalid Protected attribute: {err}"
                                ))
                            })?;
                            protected = value == "True";
                        }
                    }
                }
                Event::Empty(_) => *unsupported = true,
                Event::Eof => break,
                _ => {}
            }
        }

        // Map standard fields
        match key.as_str() {
            "Title" => {
                entry.title = value;
                entry.title_is_protected = protected;
            }
            "UserName" => {
                entry.username = if protected {
                    ProtectedString::new_protected(&value)
                } else {
                    ProtectedString::new_plain(&value)
                }
            }
            "Password" => {
                entry.password = if protected {
                    ProtectedString::new_protected(&value)
                } else {
                    ProtectedString::new_plain(&value)
                }
            }
            "URL" => {
                entry.url = value;
                entry.url_is_protected = protected;
            }
            "Notes" => {
                entry.notes = if protected {
                    ProtectedString::new_protected(&value)
                } else {
                    ProtectedString::new_plain(&value)
                }
            }
            _ => {
                entry.custom_fields.push(EntryField {
                    name: key,
                    value: if protected {
                        ProtectedString::new_protected(&value)
                    } else {
                        ProtectedString::new_plain(&value)
                    },
                    is_protected: protected,
                });
            }
        }
        Ok(())
    }

    fn read_entry_binary<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        entry: &mut Entry,
        binaries: &[(Vec<u8>, bool)],
        unsupported: &mut bool,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        let mut key = String::new();
        let mut value = Vec::new();
        let mut is_protected = false;

        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "Key" => key = read_text_content(r, buf)?,
                    "Value" => {
                        let mut reference = None;
                        for attr in e.attributes() {
                            let attr = attr?;
                            if attr.key.as_ref() == b"Ref" {
                                reference = Some(
                                    std::str::from_utf8(&attr.value)
                                        .map_err(|err| {
                                            DatabaseError::InvalidFormat(format!(
                                                "invalid binary Ref: {err}"
                                            ))
                                        })?
                                        .parse::<usize>()
                                        .map_err(|err| {
                                            DatabaseError::InvalidFormat(format!(
                                                "invalid binary Ref: {err}"
                                            ))
                                        })?,
                                );
                            }
                        }
                        let text = read_text_content(r, buf)?;
                        if let Some(index) = reference {
                            let binary = binaries.get(index).ok_or_else(|| {
                                DatabaseError::InvalidFormat(format!(
                                    "binary Ref {index} is out of range"
                                ))
                            })?;
                            value = binary.0.clone();
                            is_protected = binary.1;
                        } else {
                            value = base64::engine::general_purpose::STANDARD
                                .decode(text.trim())
                                .map_err(|err| {
                                    DatabaseError::InvalidFormat(format!(
                                        "invalid binary base64: {err}"
                                    ))
                                })?;
                        }
                    }
                    _ => {
                        *unsupported = true;
                        skip_element(r, e.name().as_ref())?;
                    }
                },
                Event::Empty(e) if tag(&e) == "Value" => {
                    let attr = e
                        .attributes()
                        .find(|a| {
                            a.as_ref()
                                .map(|a| a.key.as_ref() == b"Ref")
                                .unwrap_or(false)
                        })
                        .ok_or_else(|| {
                            DatabaseError::InvalidFormat("empty binary Value has no Ref".into())
                        })??;
                    let index = std::str::from_utf8(&attr.value)
                        .map_err(|err| {
                            DatabaseError::InvalidFormat(format!("invalid binary Ref: {err}"))
                        })?
                        .parse::<usize>()
                        .map_err(|err| {
                            DatabaseError::InvalidFormat(format!("invalid binary Ref: {err}"))
                        })?;
                    let binary = binaries.get(index).ok_or_else(|| {
                        DatabaseError::InvalidFormat(format!("binary Ref {index} is out of range"))
                    })?;
                    value = binary.0.clone();
                    is_protected = binary.1;
                }
                Event::End(e) if tag_end(&e) == "Binary" => break,
                Event::Empty(_) => *unsupported = true,
                Event::Eof => break,
                _ => {}
            }
        }

        if !key.is_empty() {
            entry.binaries.push(EntryBinary {
                name: key,
                data: value,
                is_protected,
            });
        }
        Ok(())
    }

    fn read_auto_type<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        entry: &mut Entry,
        unsupported: &mut bool,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "Enabled" => entry.auto_type.enabled = read_text_content(r, buf)? != "False",
                    "DefaultSequence" => {
                        entry.auto_type.default_sequence = read_text_content(r, buf)?
                    }
                    "Association" => Self::read_auto_type_association(r, entry, unsupported, buf)?,
                    _ => {
                        *unsupported = true;
                        skip_element(r, e.name().as_ref())?;
                    }
                },
                Event::End(e) if tag_end(&e) == "AutoType" => return Ok(()),
                Event::Empty(e) => match tag(&e).as_str() {
                    "Enabled" | "DefaultSequence" => {}
                    _ => *unsupported = true,
                },
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }

    fn read_auto_type_association<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        entry: &mut Entry,
        unsupported: &mut bool,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        let mut association = AutoTypeAssociation {
            window_title: String::new(),
            keystroke_sequence: String::new(),
        };
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "Window" => association.window_title = read_text_content(r, buf)?,
                    "KeystrokeSequence" => {
                        association.keystroke_sequence = read_text_content(r, buf)?
                    }
                    _ => {
                        *unsupported = true;
                        skip_element(r, e.name().as_ref())?;
                    }
                },
                Event::End(e) if tag_end(&e) == "Association" => {
                    entry.auto_type.associations.push(association);
                    return Ok(());
                }
                Event::Empty(e) => match tag(&e).as_str() {
                    "Window" | "KeystrokeSequence" => {}
                    _ => *unsupported = true,
                },
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }

    fn read_entry_times<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        entry: &mut Entry,
        db: &mut Database,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) => match tag(&e).as_str() {
                    "CreationTime" => {
                        entry.creation_time = date_from_xml(&read_text_content(r, buf)?)?
                    }
                    "LastModificationTime" => {
                        entry.last_modification_time = date_from_xml(&read_text_content(r, buf)?)?
                    }
                    "LastAccessTime" => {
                        entry.last_access_time = date_from_xml(&read_text_content(r, buf)?)?
                    }
                    "LocationChanged" => {
                        entry.location_changed = date_from_xml(&read_text_content(r, buf)?)?
                    }
                    "ExpiryTime" => entry.expiry_time = date_from_xml(&read_text_content(r, buf)?)?,
                    "Expires" => entry.expires = read_text_content(r, buf)? == "True",
                    "UsageCount" => {
                        entry.usage_count = read_text_content(r, buf)?.parse().map_err(|err| {
                            DatabaseError::InvalidFormat(format!(
                                "invalid entry usage count: {err}"
                            ))
                        })?
                    }
                    _ => {
                        db.contains_unsupported_xml = true;
                        skip_element(r, e.name().as_ref())?;
                    }
                },
                Event::End(e) if tag_end(&e) == "Times" => return Ok(()),
                Event::Empty(e) => match tag(&e).as_str() {
                    "CreationTime"
                    | "LastModificationTime"
                    | "LastAccessTime"
                    | "LocationChanged"
                    | "ExpiryTime" => {}
                    _ => db.contains_unsupported_xml = true,
                },
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }

    fn read_history<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        entry: &mut Entry,
        db: &mut Database,
        is: &mut dyn InnerStreamCipher,
        binaries: &[(Vec<u8>, bool)],
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) if tag(&e) == "Entry" => {
                    let h = Self::read_entry(r, db, is, binaries, buf)?;
                    entry.history.push(h);
                }
                Event::End(e) if tag_end(&e) == "History" => return Ok(()),
                Event::Start(e) => {
                    db.contains_unsupported_xml = true;
                    skip_element(r, e.name().as_ref())?;
                }
                Event::Empty(_) => db.contains_unsupported_xml = true,
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
                    let mut id = None;
                    let mut deletion_time = 0;
                    loop {
                        buf.clear();
                        match r.read_event_into(buf)? {
                            Event::Start(e2) => match tag(&e2).as_str() {
                                "UUID" => {
                                    let s = read_text_content(r, buf)?;
                                    id = Some(NodeId::from_uuid(required_uuid_from_b64(
                                        &s,
                                        "deleted object",
                                    )?));
                                }
                                "DeletionTime" => {
                                    deletion_time = date_from_xml(&read_text_content(r, buf)?)?
                                        .as_millis()
                                        .ok_or_else(|| {
                                            DatabaseError::InvalidFormat(
                                                "deleted object is missing deletion time".into(),
                                            )
                                        })?
                                }
                                _ => {
                                    db.contains_unsupported_xml = true;
                                    skip_element(r, e2.name().as_ref())?;
                                }
                            },
                            Event::End(e2) if tag_end(&e2) == "DeletedObject" => break,
                            Event::Empty(_) => db.contains_unsupported_xml = true,
                            Event::Eof => break,
                            _ => {}
                        }
                    }
                    db.deleted_objects.push(DeletedObject {
                        id: id.ok_or_else(|| {
                            DatabaseError::InvalidFormat("deleted object is missing UUID".into())
                        })?,
                        deletion_time,
                    });
                }
                Event::End(e) if tag_end(&e) == "DeletedObjects" => return Ok(()),
                Event::Start(e) => {
                    db.contains_unsupported_xml = true;
                    skip_element(r, e.name().as_ref())?;
                }
                Event::Empty(_) => db.contains_unsupported_xml = true,
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }

    fn read_custom_data<R: std::io::BufRead>(
        r: &mut quick_xml::Reader<R>,
        data: &mut CustomData,
        unsupported: &mut bool,
        buf: &mut Vec<u8>,
    ) -> DatabaseResult<()> {
        loop {
            buf.clear();
            match r.read_event_into(buf)? {
                Event::Start(e) if tag(&e) == "Item" => {
                    let mut key = String::new();
                    let mut value = String::new();
                    let mut last_modification_time = None;
                    loop {
                        buf.clear();
                        match r.read_event_into(buf)? {
                            Event::Start(item) => match tag(&item).as_str() {
                                "Key" => key = read_text_content(r, buf)?,
                                "Value" => value = read_text_content(r, buf)?,
                                "LastModificationTime" => {
                                    last_modification_time =
                                        date_from_xml(&read_text_content(r, buf)?)?.as_millis()
                                }
                                _ => {
                                    *unsupported = true;
                                    skip_element(r, item.name().as_ref())?;
                                }
                            },
                            Event::End(item) if tag_end(&item) == "Item" => break,
                            Event::Empty(item) => match tag(&item).as_str() {
                                "Key" | "Value" | "LastModificationTime" => {}
                                _ => *unsupported = true,
                            },
                            Event::Eof => break,
                            _ => {}
                        }
                    }
                    data.insert(
                        key,
                        CustomDataItem {
                            value,
                            last_modification_time,
                        },
                    );
                }
                Event::End(e) if tag_end(&e) == "CustomData" => return Ok(()),
                Event::Start(e) => {
                    *unsupported = true;
                    skip_element(r, e.name().as_ref())?;
                }
                Event::Empty(_) => *unsupported = true,
                Event::Eof => return Ok(()),
                _ => {}
            }
        }
    }
}
