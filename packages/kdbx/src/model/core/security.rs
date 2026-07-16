//! Security types
//!

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// A string that may be memory-protected (encrypted in memory).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtectedString {
    /// Plain text (not protected)
    Plain(String),
    /// Protected (will be encrypted in memory at rest)
    Protected(String),
}

impl ProtectedString {
    /// Create a new empty protected string
    pub fn new() -> Self {
        Self::Protected(String::new())
    }

    /// Create a plain (non-protected) string
    pub fn new_plain(s: &str) -> Self {
        Self::Plain(s.to_string())
    }

    /// Create a protected string
    pub fn new_protected(s: &str) -> Self {
        Self::Protected(s.to_string())
    }

    /// Check if the string is protected
    pub fn is_protected(&self) -> bool {
        matches!(self, ProtectedString::Protected(_))
    }

    /// Get the string value
    pub fn as_str(&self) -> &str {
        match self {
            ProtectedString::Plain(s) => s,
            ProtectedString::Protected(s) => s,
        }
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.as_str().is_empty()
    }

    /// Read raw data as bytes
    pub fn as_bytes(&self) -> &[u8] {
        self.as_str().as_bytes()
    }
}

impl Default for ProtectedString {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ProtectedString {
    fn drop(&mut self) {
        match self {
            ProtectedString::Plain(s) => s.zeroize(),
            ProtectedString::Protected(s) => s.zeroize(),
        }
    }
}

/// Memory protection configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryProtectionConfig {
    pub protect_title: bool,
    pub protect_username: bool,
    pub protect_password: bool,
    pub protect_url: bool,
    pub protect_notes: bool,
    pub auto_enable_visual_hiding: bool,
}
