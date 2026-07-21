use crate::crypto::memory_protection::MemoryUnlockSession;
use crate::model::{
    CompositeKey, Database, Entry, EntryField, EntryFieldId, EntryFieldUpdate, NodeId,
    ProtectedString, Template, TemplateField, TemplateFieldType,
};
use crate::{DatabaseError, DatabaseResult};

use super::metadata::{self, FieldType};

pub fn is_internal_field(name: &str) -> bool {
    name.starts_with(metadata::PREFIX)
}

pub fn visible_fields(entry: &Entry) -> impl Iterator<Item = (EntryFieldId, &EntryField)> {
    entry
        .fields()
        .filter(|(_, field)| !is_internal_field(field.name()))
}

pub fn is_template(database: &Database, entry_id: &NodeId) -> bool {
    template_entry(database, entry_id).is_some()
}

fn template_entry<'a>(database: &'a Database, entry_id: &NodeId) -> Option<&'a Entry> {
    entry_id.as_uuid()?;
    let Some(group_id) = database.entry_templates_uuid.map(NodeId::from_uuid) else {
        return None;
    };
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

pub fn instantiate(
    database: &mut Database,
    source_entry_id: &NodeId,
    parent_group_id: &NodeId,
    composite_key: Option<&CompositeKey>,
) -> DatabaseResult<Option<NodeId>> {
    if !database.groups.contains_key(parent_group_id) || !is_template(database, source_entry_id) {
        return Ok(None);
    }
    let source = template_entry(database, source_entry_id).expect("eligible template exists");
    let Some(source_uuid) = source.id.as_uuid().copied() else {
        return Ok(None);
    };
    let mut entry = source.clone();
    let id = NodeId::new_uuid();
    let mut unlock = composite_key.map(MemoryUnlockSession::new);
    entry.prepare_duplicate(id, unlock.as_mut())?;
    entry.retain_custom_fields(|field| !is_internal_field(field.name()));
    entry.add_custom_field(
        metadata::TEMPLATE_UUID,
        ProtectedString::new_plain(&source_uuid.simple().to_string().to_ascii_uppercase()),
    );
    Ok(database.add_entry(entry, parent_group_id).then_some(id))
}

pub(super) fn resolve<'a>(database: &'a Database, entry: &Entry) -> Option<&'a Entry> {
    let id = NodeId::from_uuid(metadata::template_uuid(entry)?);
    is_template(database, &id)
        .then(|| database.get_entry(&id))
        .flatten()
}

pub fn merge_metadata_updates(
    original: &Entry,
    fields: &[EntryFieldUpdate],
) -> DatabaseResult<Vec<EntryFieldUpdate>> {
    for requested in fields {
        if is_internal_field(&requested.name)
            || requested.field_id.is_some_and(|id| {
                original
                    .field(id)
                    .is_some_and(|field| is_internal_field(field.name()))
            })
        {
            return Err(DatabaseError::InvalidFormat(
                "internal template fields cannot be updated".into(),
            ));
        }
    }

    let mut visible = fields.iter();
    let mut complete = Vec::with_capacity(original.fields().count() + fields.len());
    for (id, field) in original.fields() {
        if is_internal_field(field.name()) {
            complete.push(EntryFieldUpdate {
                field_id: Some(id),
                name: field.name().to_string(),
                value: (!field.value().is_protected()).then(|| field.value().as_str().to_string()),
                is_protected: field.value().is_protected(),
            });
        } else if let Some(field) = visible.next() {
            complete.push(field.clone());
        }
    }
    complete.extend(visible.cloned());
    Ok(complete)
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
