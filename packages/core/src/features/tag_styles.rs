use std::collections::BTreeMap;

use keeless_kdbx::Database;
use keeless_schema::TagStyle;

use crate::model::parse_icon_reference;
use crate::{CoreError, Result};

pub(crate) const CUSTOM_DATA_KEY: &str = "KLSS_TAG_STYLES";

pub(crate) fn load(database: &Database) -> Result<BTreeMap<String, TagStyle>> {
    let Some(value) = database.custom_data.get(CUSTOM_DATA_KEY) else {
        return Ok(BTreeMap::new());
    };
    let mut styles: BTreeMap<String, TagStyle> =
        serde_json::from_str(value).map_err(|_| CoreError::MalformedTagStyles)?;
    for (name, style) in &mut styles {
        if name.is_empty() || name.trim() != name {
            return Err(CoreError::MalformedTagStyles);
        }
        normalize_style(database, style).map_err(|_| CoreError::MalformedTagStyles)?;
    }
    Ok(styles)
}

pub(crate) fn normalize_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CoreError::InvalidTagName);
    }
    Ok(name)
}

pub(crate) fn normalize_style(database: &Database, style: &mut TagStyle) -> Result<()> {
    if style.color.len() != 7
        || !style.color.starts_with('#')
        || !style.color[1..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(CoreError::InvalidTagStyle);
    }
    parse_icon_reference(database, &style.icon)?;
    style.color.make_ascii_lowercase();
    Ok(())
}

pub(crate) fn is_used(database: &Database, name: &str) -> bool {
    database
        .entries
        .values()
        .any(|entry| entry.tags.iter().any(|tag| tag.trim() == name))
}
