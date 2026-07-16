//! Node types and IDs
//!
//! `NodeId` uses a custom `serde` representation so that the on-the-wire JSON
//! shape matches the de-facto project convention used by the `kdbx2json`
//! example: a `Uuid` variant serializes to a hyphenated UUID **string**, and an
//! `Int` variant serializes to a bare **number**. This keeps `Database::to_json`
//! output compact (no `{"Uuid": "..."}` wrapper) and is unambiguous to
//! deserialize — a JSON string is always parsed as a UUID, a JSON number as an
//! integer.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for a database node (Entry or Group).
///
/// Serialization shape:
/// - `NodeId::Uuid(u)`  ↔ `"a1b2c3d4-e5f6-7890-abcd-ef1234567890"` (string)
/// - `NodeId::Int(42)`  ↔ `42`                                       (number)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeId {
    /// UUID-based ID (KDBX format)
    Uuid(Uuid),
    /// Integer-based ID (KDB format)
    Int(i32),
}

impl Serialize for NodeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            NodeId::Uuid(u) => serializer.serialize_str(&u.hyphenated().to_string()),
            NodeId::Int(i) => serializer.serialize_i32(*i),
        }
    }
}

impl<'de> Deserialize<'de> for NodeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};

        /// Visitor that accepts either a UUID string or an integer.
        struct NodeIdVisitor;

        impl<'de> Visitor<'de> for NodeIdVisitor {
            type Value = NodeId;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a UUID string or an integer NodeId")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<NodeId, E> {
                Uuid::parse_str(v)
                    .map(NodeId::Uuid)
                    .map_err(|e| E::custom(format!("invalid UUID for NodeId: {e}")))
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<NodeId, E> {
                self.visit_str(&v)
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<NodeId, E> {
                i32::try_from(v)
                    .map(NodeId::Int)
                    .map_err(|_| E::custom("NodeId integer out of i32 range"))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<NodeId, E> {
                i32::try_from(v)
                    .map(NodeId::Int)
                    .map_err(|_| E::custom("NodeId integer out of i32 range"))
            }
        }

        deserializer.deserialize_any(NodeIdVisitor)
    }
}

impl NodeId {
    pub fn new_uuid() -> Self {
        Self::Uuid(Uuid::new_v4())
    }

    pub fn from_uuid(uuid: Uuid) -> Self {
        Self::Uuid(uuid)
    }

    pub fn from_int(id: i32) -> Self {
        Self::Int(id)
    }

    pub fn as_uuid(&self) -> Option<&Uuid> {
        match self {
            NodeId::Uuid(u) => Some(u),
            _ => None,
        }
    }

    /// Create from u32 (KDB format)
    pub fn from_u32(id: u32) -> Self {
        Self::Int(id as i32)
    }

    /// Get as u32 (KDB format)
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            NodeId::Int(i) => Some(*i as u32),
            _ => None,
        }
    }
}

/// Node type indicator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Group,
    Entry,
}

/// Base trait for database nodes (groups and entries).
pub trait Node: Send + Sync {
    fn node_id(&self) -> &NodeId;
    fn node_type(&self) -> NodeType;
    fn title(&self) -> &str;
    fn last_modified(&self) -> i64;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_uuid_serializes_as_hyphenated_string() {
        let id = NodeId::Uuid(Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567890").unwrap());
        let s = serde_json::to_string(&id).unwrap();
        assert_eq!(s, "\"a1b2c3d4-e5f6-7890-abcd-ef1234567890\"");
    }

    #[test]
    fn node_id_int_serializes_as_bare_number() {
        let id = NodeId::Int(42);
        let s = serde_json::to_string(&id).unwrap();
        assert_eq!(s, "42");
    }

    #[test]
    fn node_id_uuid_roundtrips_through_json() {
        let id = NodeId::new_uuid();
        let s = serde_json::to_string(&id).unwrap();
        let back: NodeId = serde_json::from_str(&s).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn node_id_int_roundtrips_through_json() {
        let id = NodeId::Int(-7);
        let s = serde_json::to_string(&id).unwrap();
        let back: NodeId = serde_json::from_str(&s).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn node_id_int_accepts_u32_input_range() {
        // KDB IDs originate as u32; verify a large positive integer deserializes.
        let back: NodeId = serde_json::from_str("2147483647").unwrap();
        assert_eq!(back, NodeId::Int(i32::MAX));
    }

    #[test]
    fn node_id_rejects_invalid_uuid_string() {
        assert!(serde_json::from_str::<NodeId>("\"not-a-uuid\"").is_err());
    }

    #[test]
    fn node_id_rejects_out_of_range_integer() {
        assert!(serde_json::from_str::<NodeId>("9999999999").is_err());
    }
}
