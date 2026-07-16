//! Icon data model
//!

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Standard icon index (0-68)
pub const NUMBER_STANDARD_ICONS: usize = 69;

/// Icon representation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IconImage {
    /// Standard KeePass icon (index 0-68)
    Standard(IconImageStandard),
    /// Custom icon (UUID-based)
    Custom(IconImageCustom),
}

impl Default for IconImage {
    fn default() -> Self {
        IconImage::Standard(IconImageStandard::default())
    }
}

/// Standard icon reference
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct IconImageStandard {
    pub icon_id: u32,
}

impl IconImageStandard {
    pub fn new(icon_id: u32) -> Self {
        Self { icon_id: icon_id.min(NUMBER_STANDARD_ICONS as u32 - 1) }
    }
}

/// Custom icon reference
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconImageCustom {
    pub uuid: Uuid,
    pub data: Vec<u8>,
    pub name: String,
    pub last_modification_time: i64,
}

impl IconImageCustom {
    pub fn new(uuid: Uuid, data: Vec<u8>) -> Self {
        Self {
            uuid,
            data,
            name: String::new(),
            last_modification_time: 0,
        }
    }
}
