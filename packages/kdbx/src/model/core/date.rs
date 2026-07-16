//! Date/time instant representation
//!

use serde::{Deserialize, Serialize};

/// Represents a point in time for KeePass database entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DateInstant {
    /// System.currentTimeMillis() based timestamp
    EpochMillis(i64),
    /// KeePass compressed date (5 bytes)
    Compressed([u8; 5]),
    /// Never expires
    #[default]
    Never,
}

impl DateInstant {
    pub fn now() -> Self {
        Self::EpochMillis(chrono::Utc::now().timestamp_millis())
    }

    pub fn never() -> Self {
        Self::Never
    }

    pub fn is_never(&self) -> bool {
        matches!(self, DateInstant::Never)
    }

    /// Get epoch millis if available
    pub fn as_millis(&self) -> Option<i64> {
        match self {
            DateInstant::EpochMillis(ms) => Some(*ms),
            _ => None,
        }
    }
}
