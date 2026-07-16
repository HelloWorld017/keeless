//! AutoType configuration
//!

use serde::{Deserialize, Serialize};

/// Auto-type association (window title → keystroke sequence)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoTypeAssociation {
    pub window_title: String,
    pub keystroke_sequence: String,
}

/// Auto-type configuration for an entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoType {
    pub enabled: bool,
    pub default_sequence: String,
    pub associations: Vec<AutoTypeAssociation>,
}

impl AutoType {
    pub fn new() -> Self {
        Self {
            enabled: true,
            default_sequence: String::new(),
            associations: Vec::new(),
        }
    }

    /// Get the keystroke sequence for a given window title.
    pub fn get_sequence_for_window(&self, window_title: &str) -> Option<&str> {
        for assoc in &self.associations {
            if window_title.contains(&assoc.window_title) {
                return Some(&assoc.keystroke_sequence);
            }
        }
        if !self.default_sequence.is_empty() {
            Some(&self.default_sequence)
        } else {
            None
        }
    }
}

impl Default for AutoType {
    fn default() -> Self {
        Self::new()
    }
}
