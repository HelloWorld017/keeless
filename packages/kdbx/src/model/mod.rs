//! Data models for KeePass database elements
//!
//!
//! ## Structure
//! - `db` — Database core, composite key, change tracking
//! - `binary` — Binary pool, cache, stream I/O
//! - `core` — Nodes, dates, security, sorting, traversal
//! - `entry` — Entry model, auto-type, field references, versioned fields
//! - `group` — Group model, versioned fields
//! - `meta` — Icons, tags, custom data, templates, deleted objects

pub mod binary;
pub mod core;
pub mod db;
pub mod entry;
pub mod exception;
pub mod group;
pub mod meta;

// Re-export key types
pub use db::{Database, DatabaseVersion, CompositeKey, MasterCredential, ChangeTracker, ChangeType, ChangeRecord, DiffResult};
pub use entry::{Entry, EntryField};
pub use entry::auto_type::{AutoType, AutoTypeAssociation};
pub use entry::field_references::{FieldReference, RefTarget};
pub use entry::versioned::{EntryKDB, EntryKDBX};
pub use group::Group;
pub use group::versioned::{GroupKDB, GroupKDBX};
pub use core::{Node, NodeId, NodeType, TraversalOrder, NodeHandler, DateInstant, ProtectedString, MemoryProtectionConfig, SortNodeEnum};
pub use binary::{BinaryData, BinaryPool, BinaryCache, BinaryStreamReader, BinaryStreamWriter};
pub use meta::{IconImage, IconImageStandard, IconImageCustom, Tag, parse_tags, serialize_tags, CustomData, CustomDataItem, DeletedObject, Template, TemplateField, TemplateFieldType};
