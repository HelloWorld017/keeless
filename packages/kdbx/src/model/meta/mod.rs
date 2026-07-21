//! Metadata: icons, tags, custom data, templates, and deleted objects.

pub mod custom_data;
pub mod deleted_object;
pub mod etm;
pub mod icon;
pub mod tags;
pub mod template;

pub use custom_data::{CustomData, CustomDataItem};
pub use deleted_object::DeletedObject;
pub use etm::{
    parse_etm_template, EtmField, EtmFieldType, EtmTarget, EtmTemplate, ETM_OPTIONS_PREFIX,
    ETM_POSITION_PREFIX, ETM_PREFIX, ETM_TEMPLATE, ETM_TEMPLATE_UUID, ETM_TITLE_PREFIX,
    ETM_TYPE_PREFIX,
};
pub use icon::{IconImage, IconImageCustom, IconImageStandard};
pub use tags::{parse_tags, serialize_tags, Tag};
pub use template::{get_builtin_templates, Template, TemplateField, TemplateFieldType};
