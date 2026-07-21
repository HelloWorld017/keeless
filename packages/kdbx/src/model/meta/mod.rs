//! Metadata: icons, tags, custom data, templates, and deleted objects.

pub mod custom_data;
pub mod deleted_object;
pub mod icon;
pub mod tags;
pub mod template;

pub use custom_data::{CustomData, CustomDataItem};
pub use deleted_object::DeletedObject;
pub use icon::{IconImage, IconImageCustom, IconImageStandard};
pub use tags::{parse_tags, serialize_tags, Tag};
pub use template::{get_builtin_templates, Template, TemplateField, TemplateFieldType};
