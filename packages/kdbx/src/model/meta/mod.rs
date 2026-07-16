//! Metadata: icons, tags, custom data, templates, and deleted objects.

pub mod custom_data;
pub mod deleted_object;
pub mod icon;
pub mod tags;
pub mod template;

pub use icon::{IconImage, IconImageStandard, IconImageCustom};
pub use tags::{Tag, parse_tags, serialize_tags};
pub use custom_data::{CustomData, CustomDataItem};
pub use deleted_object::DeletedObject;
pub use template::{Template, TemplateField, TemplateFieldType};
