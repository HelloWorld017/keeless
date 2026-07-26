//! Deleted object tracking for KDBX 4.0 recycle bin
//!

use serde::{Deserialize, Serialize};

use crate::model::core::node::NodeId;

/// A deleted object record (KDBX 4.0).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletedObject {
    pub id: NodeId,
    pub deletion_time: i64,
}

impl DeletedObject {
    pub fn new(id: NodeId) -> Self {
        Self::new_at(id, chrono::Utc::now().timestamp_millis())
    }

    pub fn new_at(id: NodeId, deletion_time: i64) -> Self {
        Self { id, deletion_time }
    }
}
