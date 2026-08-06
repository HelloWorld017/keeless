//! KeePassXC enhanced entry-template wire support.

mod entry;
mod layout;
mod metadata;

pub use entry::{
    builtin_entry, commit_instantiation, entries, instantiate, instantiate_at, is_template,
    is_template_field, prepare_instantiation_at, PreparedTemplateInstantiation, TemplateCopyMode,
    TemplateInstantiationOptions,
};
pub use layout::{resolve_layout, EntryLayout, EntryLayoutItem, FieldControl, LayoutTarget};
