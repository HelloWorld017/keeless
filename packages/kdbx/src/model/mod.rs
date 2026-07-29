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
#[doc(hidden)]
pub mod xml;

// Re-export key types
pub use binary::{BinaryCache, BinaryData, BinaryPool, BinaryStreamReader, BinaryStreamWriter};
pub use core::{
    DateInstant, MemoryProtectionConfig, Node, NodeHandler, NodeId, NodeType, ProtectedString,
    SortNodeEnum, TraversalOrder,
};
pub use db::{
    ChangeRecord, ChangeTracker, ChangeType, CompositeKey, Database, DatabaseVersion, DiffResult,
    EntryFieldSelector, EntryFieldUpdate, EntryPropertiesUpdate, EntryUpdate, IconUpdate,
    MasterCredential, PreparedEntryUpdate,
};
pub use entry::auto_type::{AutoType, AutoTypeAssociation};
pub use entry::field_references::{FieldReference, RefTarget};
pub use entry::versioned::{EntryKDB, EntryKDBX};
pub use entry::{Entry, EntryBinary, EntryField, EntryFieldId, StandardField};
pub use group::versioned::{GroupKDB, GroupKDBX};
pub use group::Group;
pub use meta::{
    get_builtin_templates, parse_tags, serialize_tags, CustomData, CustomDataItem, DeletedObject,
    IconImage, IconImageCustom, IconImageStandard, Tag, Template, TemplateField, TemplateFieldType,
    NUMBER_STANDARD_ICONS,
};
