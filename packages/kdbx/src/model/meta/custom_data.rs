//! Custom data key-value pairs
//!

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single custom data item with optional last modification info.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomDataItem {
    pub value: String,
    pub last_modification_time: Option<i64>,
}

/// Custom data dictionary (string → string with timestamps).
/// Used in KDBX 4.0 for extensible metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomData {
    items: HashMap<String, CustomDataItem>,
}

impl CustomData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.items.get(key).map(|i| i.value.as_str())
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.items.insert(
            key.to_string(),
            CustomDataItem {
                value: value.to_string(),
                last_modification_time: Some(chrono::Utc::now().timestamp_millis()),
            },
        );
    }

    /// Insert an item while preserving its serialized modification time.
    pub fn insert(&mut self, key: String, item: CustomDataItem) {
        self.items.insert(key, item);
    }

    pub fn remove(&mut self, key: &str) {
        self.items.remove(key);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &CustomDataItem)> {
        self.items.iter()
    }
}
