//! KeePassXC enhanced entry-template wire support.

mod entry;
mod layout;
mod metadata;

pub use entry::{
    builtin_entry, entries, instantiate, is_internal_field, is_template, merge_metadata_updates,
    visible_fields,
};
pub use layout::{resolve_layout, EntryLayout, EntryLayoutItem, FieldControl, LayoutTarget};
