use crate::crypto::memory_protection::MemoryUnlockSession;
use crate::model::{
    CompositeKey, Database, DateInstant, Entry, EntryFieldId, NodeId, ProtectedString,
    StandardField, Template, TemplateField, TemplateFieldType,
};
use crate::{DatabaseError, DatabaseResult};
use indexmap::IndexMap;
use uuid::Uuid;

use super::metadata::{self, FieldType};

pub fn is_internal_field(name: &str) -> bool {
    name.starts_with(metadata::PREFIX)
}

pub fn is_template(database: &Database, entry_id: &NodeId) -> bool {
    template_entry(database, entry_id).is_some()
}

fn template_entry<'a>(database: &'a Database, entry_id: &NodeId) -> Option<&'a Entry> {
    entry_id.as_uuid()?;
    let group_id = database.entry_templates_uuid.map(NodeId::from_uuid)?;
    if !database
        .get_group(&group_id)?
        .child_entry_ids
        .contains(entry_id)
    {
        return None;
    }
    database
        .get_entry(entry_id)
        .filter(|entry| entry.id == *entry_id && metadata::has_marker(entry))
}

pub fn entries(database: &Database) -> Vec<&Entry> {
    let Some(group_id) = database.entry_templates_uuid.map(NodeId::from_uuid) else {
        return Vec::new();
    };
    database
        .get_group(&group_id)
        .into_iter()
        .flat_map(|group| &group.child_entry_ids)
        .filter_map(|entry_id| template_entry(database, entry_id))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateCopyMode {
    PreserveProtected,
    RedactProtected,
}

pub struct TemplateInstantiationOptions<'a> {
    pub new_entry_id: NodeId,
    pub link_field_id: EntryFieldId,
    pub timestamp: DateInstant,
    pub copy_mode: TemplateCopyMode,
    pub composite_key: Option<&'a CompositeKey>,
}

/// A fully prepared template entry insertion with an infallible commit path.
pub struct PreparedTemplateInstantiation {
    entry: Entry,
    parent_group_id: NodeId,
}

pub fn prepare_instantiation_at(
    database: &Database,
    source_entry_id: &NodeId,
    parent_group_id: &NodeId,
    options: TemplateInstantiationOptions<'_>,
) -> DatabaseResult<Option<PreparedTemplateInstantiation>> {
    if !database.can_add_entry(&options.new_entry_id, parent_group_id)
        || !is_template(database, source_entry_id)
        || !matches!(options.link_field_id, EntryFieldId::Custom(_))
    {
        return Ok(None);
    }
    let source = template_entry(database, source_entry_id).expect("eligible template exists");
    let Some(source_uuid) = source.id.as_uuid().copied() else {
        return Ok(None);
    };
    if source.field(options.link_field_id).is_some() {
        return Ok(None);
    }
    let mut entry = source.clone();
    let mut unlock = match options.copy_mode {
        TemplateCopyMode::PreserveProtected => Some(MemoryUnlockSession::new(
            options.composite_key.ok_or_else(|| {
                DatabaseError::InvalidFormat(
                    "preserving protected template fields requires credentials".into(),
                )
            })?,
        )),
        TemplateCopyMode::RedactProtected => None,
    };
    entry.prepare_duplicate_at(options.new_entry_id, unlock.as_mut(), options.timestamp)?;
    entry.retain_custom_fields(|field| {
        !is_internal_field(field.name()) && !field.name().starts_with('@')
    });
    standard_fields_first(&mut entry);
    entry.add_custom_field_with_id(
        options.link_field_id,
        metadata::TEMPLATE_UUID,
        ProtectedString::new_plain(&source_uuid.simple().to_string().to_ascii_uppercase()),
    );
    Ok(Some(PreparedTemplateInstantiation {
        entry,
        parent_group_id: *parent_group_id,
    }))
}

pub fn commit_instantiation(
    database: &mut Database,
    prepared: PreparedTemplateInstantiation,
) -> NodeId {
    let id = prepared.entry.id;
    database.add_entry_validated(prepared.entry, &prepared.parent_group_id);
    id
}

pub fn instantiate(
    database: &mut Database,
    source_entry_id: &NodeId,
    parent_group_id: &NodeId,
    composite_key: Option<&CompositeKey>,
) -> DatabaseResult<Option<NodeId>> {
    let copy_mode = if composite_key.is_some() {
        TemplateCopyMode::PreserveProtected
    } else {
        TemplateCopyMode::RedactProtected
    };
    instantiate_at(
        database,
        source_entry_id,
        parent_group_id,
        TemplateInstantiationOptions {
            new_entry_id: NodeId::new_uuid(),
            link_field_id: EntryFieldId::Custom(Uuid::new_v4()),
            timestamp: DateInstant::now(),
            copy_mode,
            composite_key,
        },
    )
}

/// Instantiate a template with all replay-visible identities and time supplied by the caller.
pub fn instantiate_at(
    database: &mut Database,
    source_entry_id: &NodeId,
    parent_group_id: &NodeId,
    options: TemplateInstantiationOptions<'_>,
) -> DatabaseResult<Option<NodeId>> {
    Ok(
        prepare_instantiation_at(database, source_entry_id, parent_group_id, options)?
            .map(|prepared| commit_instantiation(database, prepared)),
    )
}

fn standard_fields_first(entry: &mut Entry) {
    let mut ordered = IndexMap::with_capacity(entry.fields.0.len());
    for standard in StandardField::ALL {
        let id = EntryFieldId::Standard(standard);
        if let Some(field) = entry.fields.0.shift_remove(&id) {
            ordered.insert(id, field);
        }
    }
    ordered.append(&mut entry.fields.0);
    entry.fields.0 = ordered;
}

pub(super) fn resolve<'a>(database: &'a Database, entry: &Entry) -> Option<&'a Entry> {
    let id = NodeId::from_uuid(metadata::template_uuid(entry)?);
    is_template(database, &id)
        .then(|| database.get_entry(&id))
        .flatten()
}

pub fn builtin_entry(template: Template) -> Entry {
    let fields = template.fields.clone();
    let mut entry = template.into_entry();
    entry.add_custom_field(metadata::MARKER, ProtectedString::new_plain("1"));
    for (position, field) in fields.iter().enumerate() {
        decorate_field(&mut entry, field, position);
    }
    entry
}

fn decorate_field(entry: &mut Entry, field: &TemplateField, position: usize) {
    let field_type = if field.is_protected {
        FieldType::ProtectedInline
    } else {
        match field.field_type {
            TemplateFieldType::Url => FieldType::InlineUrl,
            TemplateFieldType::Date => FieldType::Date,
            TemplateFieldType::Toggle => FieldType::Checkbox,
            TemplateFieldType::List => FieldType::Listbox,
            TemplateFieldType::Text if field.name == "Notes" => FieldType::RichTextbox,
            _ => FieldType::Inline,
        }
    };
    for (prefix, value) in [
        (metadata::TITLE_PREFIX, field.name.clone()),
        (metadata::TYPE_PREFIX, field_type.as_str().to_string()),
        (metadata::POSITION_PREFIX, position.to_string()),
        (metadata::OPTIONS_PREFIX, String::new()),
    ] {
        entry.add_custom_field(
            format!("{prefix}{}", field.name),
            ProtectedString::new_plain(&value),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DatabaseVersion, Group};

    fn database() -> (Database, NodeId, NodeId) {
        let mut database = Database::new(DatabaseVersion::KDBX4);
        let root_id = NodeId::new_uuid();
        let templates_id = NodeId::new_uuid();
        database.groups.insert(root_id, Group::new(root_id));
        database.root_group_id = Some(root_id);
        database.add_group(Group::new(templates_id), &root_id);
        database.entry_templates_uuid = templates_id.as_uuid().copied();
        (database, root_id, templates_id)
    }

    #[test]
    fn instantiation_requires_direct_strict_template_and_replaces_metadata_with_link() {
        let (mut database, root_id, templates_id) = database();
        let source_id = NodeId::new_uuid();
        let mut source = Entry::new(source_id);
        source.set_title("Template title");
        source.add_custom_field("Ordinary", ProtectedString::new_plain("default"));
        source.add_custom_field("@exp_date", ProtectedString::new_plain("placeholder"));
        source.add_custom_field("@confirm", ProtectedString::new_plain("placeholder"));
        source.add_custom_field("@future", ProtectedString::new_plain("placeholder"));
        source.add_custom_field(metadata::MARKER, ProtectedString::new_plain("1"));
        source.add_custom_field(
            format!("{}Ordinary", metadata::TITLE_PREFIX),
            ProtectedString::new_plain("Label"),
        );
        assert!(database.add_entry(source, &templates_id));

        let child_id = instantiate(&mut database, &source_id, &root_id, None)
            .unwrap()
            .unwrap();
        let child = database.get_entry(&child_id).unwrap();
        assert_eq!(child.title().as_str(), "Template title");
        assert_eq!(
            child
                .fields()
                .map(|(_, field)| field.name())
                .collect::<Vec<_>>(),
            [
                "Title",
                "UserName",
                "Password",
                "URL",
                "Notes",
                "Ordinary",
                metadata::TEMPLATE_UUID,
            ]
        );
        assert!(child.custom_fields().any(|(_, field)| {
            field.name() == metadata::TEMPLATE_UUID
                && field.value().as_str()
                    == source_id
                        .as_uuid()
                        .unwrap()
                        .simple()
                        .to_string()
                        .to_ascii_uppercase()
        }));
        assert_eq!(
            child
                .custom_fields()
                .filter(|(_, field)| is_internal_field(field.name()))
                .count(),
            1
        );
        assert_eq!(resolve(&database, child).unwrap().id, source_id);

        let count = database.entry_count();
        assert_eq!(
            instantiate(&mut database, &source_id, &NodeId::new_uuid(), None).unwrap(),
            None
        );
        database
            .get_entry_mut(&source_id)
            .unwrap()
            .add_custom_field(metadata::MARKER, ProtectedString::new_plain("1"));
        assert_eq!(
            instantiate(&mut database, &source_id, &root_id, None).unwrap(),
            None
        );
        assert_eq!(database.entry_count(), count);
    }

    #[test]
    fn marker_must_be_unique_plain_and_exact() {
        let (mut database, _, templates_id) = database();
        for marker in [
            ProtectedString::new_plain("true"),
            ProtectedString::new_protected("1"),
        ] {
            let id = NodeId::new_uuid();
            let mut entry = Entry::new(id);
            entry.add_custom_field(metadata::MARKER, marker);
            database.add_entry(entry, &templates_id);
            assert!(!is_template(&database, &id));
        }
    }
}
