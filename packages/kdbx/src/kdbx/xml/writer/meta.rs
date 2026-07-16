use super::*;

pub(super) fn write_meta(writer: &mut XmlWriter, db: &Database) -> DatabaseResult<()> {
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
        write_memory_protection(writer, db)?;
        write_custom_icons(writer, db)?;
        super::data::write_custom_data(writer, &db.custom_data)
    })
}

fn write_memory_protection(writer: &mut XmlWriter, db: &Database) -> DatabaseResult<()> {
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
        )
    })
}

fn write_custom_icons(writer: &mut XmlWriter, db: &Database) -> DatabaseResult<()> {
    if db.custom_icons.is_empty() {
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
                Ok(())
            })?;
        }
        Ok(())
    })
}
