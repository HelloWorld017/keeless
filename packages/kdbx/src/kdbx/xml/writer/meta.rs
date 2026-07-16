use super::*;

pub(super) fn write_meta(
    writer: &mut XmlWriter,
    db: &Database,
    inner_stream: &mut dyn InnerStreamCipher,
) -> DatabaseResult<()> {
    write_element(writer, "Meta", |writer| {
        write_tag(writer, "Generator", "KeePassFR Rust")?;
        write_tag(writer, "DatabaseName", &db.name)?;
        write_tag(writer, "DatabaseDescription", &db.description)?;
        write_tag(writer, "DefaultUserName", &db.default_username)?;
        if let Some(uuid) = db.recycle_bin_uuid {
            write_tag(writer, "RecycleBinUUID", &uuid_to_b64(&uuid))?;
        }
        if let Some(uuid) = db.entry_templates_uuid {
            write_tag(writer, "EntryTemplatesGroup", &uuid_to_b64(&uuid))?;
        }
        write_memory_protection(writer, db, inner_stream)?;
        write_custom_icons(writer, db, inner_stream)?;
        super::data::write_custom_data(writer, &db.custom_data, inner_stream)?;
        write_preserved_elements(writer, &db.xml_extensions.meta, inner_stream)
    })
}

fn write_memory_protection(
    writer: &mut XmlWriter,
    db: &Database,
    inner_stream: &mut dyn InnerStreamCipher,
) -> DatabaseResult<()> {
    write_element(writer, "MemoryProtection", |writer| {
        write_tag(
            writer,
            "ProtectTitle",
            bool_xml(db.memory_protection.protect_title),
        )?;
        write_tag(
            writer,
            "ProtectUserName",
            bool_xml(db.memory_protection.protect_username),
        )?;
        write_tag(
            writer,
            "ProtectPassword",
            bool_xml(db.memory_protection.protect_password),
        )?;
        write_tag(
            writer,
            "ProtectURL",
            bool_xml(db.memory_protection.protect_url),
        )?;
        write_tag(
            writer,
            "ProtectNotes",
            bool_xml(db.memory_protection.protect_notes),
        )?;
        write_tag(
            writer,
            "AutoEnableVisualHiding",
            bool_xml(db.memory_protection.auto_enable_visual_hiding),
        )?;
        write_preserved_elements(writer, &db.xml_extensions.memory_protection, inner_stream)
    })
}

fn write_custom_icons(
    writer: &mut XmlWriter,
    db: &Database,
    inner_stream: &mut dyn InnerStreamCipher,
) -> DatabaseResult<()> {
    if db.custom_icons.is_empty() && db.xml_extensions.custom_icons.is_empty() {
        return Ok(());
    }
    write_element(writer, "CustomIcons", |writer| {
        for icon in db.custom_icons.values() {
            write_element(writer, "Icon", |writer| {
                write_tag(writer, "UUID", &uuid_to_b64(&icon.uuid))?;
                write_tag(
                    writer,
                    "Data",
                    &base64::engine::general_purpose::STANDARD.encode(&icon.data),
                )?;
                if !icon.name.is_empty() {
                    write_tag(writer, "Name", &icon.name)?;
                }
                if icon.last_modification_time != 0 {
                    write_tag(
                        writer,
                        "LastModificationTime",
                        &date_to_xml(&DateInstant::EpochMillis(icon.last_modification_time)),
                    )?;
                }
                if let Some(extensions) = db.xml_extensions.custom_icon.get(&icon.uuid) {
                    write_preserved_elements(writer, extensions, inner_stream)?;
                }
                Ok(())
            })?;
        }
        write_preserved_elements(writer, &db.xml_extensions.custom_icons, inner_stream)
    })
}
