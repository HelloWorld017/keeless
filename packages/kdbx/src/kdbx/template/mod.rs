//! KeePassXC enhanced entry-template wire support.

mod entry;
mod layout;
mod metadata;

pub use entry::{
    builtin_entry, commit_instantiation, entries, instantiate, instantiate_at, is_internal_field,
    is_template, prepare_instantiation_at, PreparedTemplateInstantiation, TemplateCopyMode,
};
pub use layout::{resolve_layout, EntryLayout, EntryLayoutItem, FieldControl, LayoutTarget};
