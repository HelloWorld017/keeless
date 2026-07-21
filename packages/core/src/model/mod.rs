use std::collections::HashSet;

use keeless_kdbx::{Database, Entry, IconImage, NodeId, StandardField};
use keeless_schema::{
    DatabaseNodeId, EntryAttachmentInformation, EntryDetailResult, EntryFieldInformation,
    EntryFieldKind, EntrySummary, GroupHierarchyItem, GroupHierarchyResult, IconReference,
};
use uuid::Uuid;

use crate::{CoreError, Result};

pub(super) fn parse_node_id(id: DatabaseNodeId) -> Result<NodeId> {
    match id {
        DatabaseNodeId::Uuid(value) => Uuid::parse_str(&value)
            .map(NodeId::from_uuid)
            .map_err(|_| CoreError::InvalidNodeId),
        DatabaseNodeId::Int(value) => Ok(NodeId::from_int(value)),
    }
}

pub(super) fn node_id(id: NodeId) -> DatabaseNodeId {
    match id {
        NodeId::Uuid(value) => DatabaseNodeId::Uuid(value.hyphenated().to_string()),
        NodeId::Int(value) => DatabaseNodeId::Int(value),
    }
}

fn node_id_sort_key(id: &NodeId) -> String {
    match id {
        NodeId::Uuid(value) => format!("uuid:{value}"),
        NodeId::Int(value) => format!("int:{value:+011}"),
    }
}

fn icon_reference(icon: &IconImage, custom_icon_uuid: Option<Uuid>) -> IconReference {
    let standard_id = match icon {
        IconImage::Standard(icon) => icon.icon_id,
        IconImage::Custom(_) => 0,
    };
    let custom_uuid = custom_icon_uuid
        .or(match icon {
            IconImage::Custom(icon) => Some(icon.uuid),
            IconImage::Standard(_) => None,
        })
        .map(|value| value.hyphenated().to_string());

    IconReference {
        standard_id,
        custom_uuid,
    }
}

pub(super) fn entry_summary(entry: &Entry) -> EntrySummary {
    EntrySummary {
        id: node_id(entry.id),
        name: (!entry.title().is_protected()).then(|| entry.title().as_str().to_owned()),
        name_is_protected: entry.title().is_protected(),
        username: (!entry.username().is_protected()).then(|| entry.username().as_str().to_owned()),
        username_is_protected: entry.username().is_protected(),
        url: (!entry.url().is_protected()).then(|| entry.url().as_str().to_owned()),
        url_is_protected: entry.url().is_protected(),
        icon: icon_reference(&entry.icon, entry.custom_icon_uuid),
        tags: entry.tags.clone(),
    }
}

pub(super) fn all_entries(database: &Database, exclude_trash: bool) -> Vec<&Entry> {
    fn visit_group<'a>(
        database: &'a Database,
        group_id: &NodeId,
        excluded_groups: &HashSet<NodeId>,
        visited_groups: &mut HashSet<NodeId>,
        visited_entries: &mut HashSet<NodeId>,
        entries: &mut Vec<&'a Entry>,
    ) {
        if excluded_groups.contains(group_id) || !visited_groups.insert(*group_id) {
            return;
        }
        let Some(group) = database.get_group(group_id) else {
            return;
        };

        for entry_id in &group.child_entry_ids {
            if visited_entries.insert(*entry_id)
                && let Some(entry) = database.get_entry(entry_id)
            {
                entries.push(entry);
            }
        }
        for child_group_id in &group.child_group_ids {
            visit_group(
                database,
                child_group_id,
                excluded_groups,
                visited_groups,
                visited_entries,
                entries,
            );
        }
    }

    let mut excluded_groups = HashSet::new();
    if let Some(id) = database.entry_templates_uuid.map(NodeId::from_uuid) {
        excluded_groups.insert(id);
    }
    if exclude_trash && let Some(id) = database.recycle_bin_uuid.map(NodeId::from_uuid) {
        excluded_groups.insert(id);
    }
    let excluded_entries = excluded_groups
        .iter()
        .flat_map(|id| database.get_all_entries_in_group(id))
        .map(|entry| entry.id)
        .collect::<HashSet<_>>();
    let mut entries = Vec::with_capacity(
        database
            .entries
            .len()
            .saturating_sub(excluded_entries.len()),
    );
    let mut visited_groups = HashSet::new();
    let mut visited_entries = HashSet::new();
    if let Some(root_group_id) = &database.root_group_id {
        visit_group(
            database,
            root_group_id,
            &excluded_groups,
            &mut visited_groups,
            &mut visited_entries,
            &mut entries,
        );
    }

    let mut remaining = database
        .entries
        .keys()
        .filter(|id| !visited_entries.contains(id) && !excluded_entries.contains(id))
        .collect::<Vec<_>>();
    remaining.sort_by_key(|id| node_id_sort_key(id));
    entries.extend(
        remaining
            .into_iter()
            .filter_map(|id| database.get_entry(id)),
    );
    entries
}

pub(super) fn all_entry_summaries(database: &Database, exclude_trash: bool) -> Vec<EntrySummary> {
    all_entries(database, exclude_trash)
        .into_iter()
        .map(entry_summary)
        .collect()
}

pub(super) fn group_hierarchy(database: &Database) -> Result<GroupHierarchyResult> {
    fn visit_group(
        database: &Database,
        group_id: &NodeId,
        visited: &mut HashSet<NodeId>,
        groups: &mut Vec<GroupHierarchyItem>,
    ) {
        if !visited.insert(*group_id) {
            return;
        }
        let Some(group) = database.get_group(group_id) else {
            return;
        };

        groups.push(GroupHierarchyItem {
            id: node_id(group.id),
            name: group.title.clone(),
            icon: icon_reference(&group.icon, group.custom_icon_uuid),
            child_group_ids: group.child_group_ids.iter().copied().map(node_id).collect(),
        });
        for child_group_id in &group.child_group_ids {
            visit_group(database, child_group_id, visited, groups);
        }
    }

    let root_group_id = database.root_group_id.ok_or(CoreError::GroupNotFound)?;
    if database.get_group(&root_group_id).is_none() {
        return Err(CoreError::GroupNotFound);
    }

    let mut groups = Vec::with_capacity(database.groups.len());
    let mut visited = HashSet::new();
    visit_group(database, &root_group_id, &mut visited, &mut groups);

    let mut remaining = database
        .groups
        .keys()
        .filter(|id| !visited.contains(id))
        .collect::<Vec<_>>();
    remaining.sort_by_key(|id| node_id_sort_key(id));
    for group_id in remaining {
        visit_group(database, group_id, &mut visited, &mut groups);
    }

    Ok(GroupHierarchyResult {
        database_name: database.name.clone(),
        root_group_id: node_id(root_group_id),
        recycle_bin_id: database
            .recycle_bin_uuid
            .map(NodeId::from_uuid)
            .map(node_id),
        groups,
    })
}

pub(super) fn entry_detail(entry: &Entry) -> EntryDetailResult {
    let fields = entry
        .fields()
        .map(|(id, field)| {
            let is_protected = field.value().is_protected();
            let kind = match field.standard() {
                Some(StandardField::Title) => EntryFieldKind::Title,
                Some(StandardField::UserName) => EntryFieldKind::UserName,
                Some(StandardField::Password) => EntryFieldKind::Password,
                Some(StandardField::Url) => EntryFieldKind::Url,
                Some(StandardField::Notes) => EntryFieldKind::Notes,
                None => EntryFieldKind::Custom,
            };
            EntryFieldInformation {
                field_id: id.to_string(),
                kind,
                name: field.name().to_string(),
                value: (!is_protected).then(|| field.value().as_str().to_string()),
                is_protected,
            }
        })
        .collect();

    EntryDetailResult {
        id: node_id(entry.id),
        icon: icon_reference(&entry.icon, entry.custom_icon_uuid),
        layout: None,
        tags: entry.tags.clone(),
        fields,
        background_color: entry.background_color.clone(),
        foreground_color: entry.foreground_color.clone(),
        override_url: entry.override_url.clone(),
        creation_time_ms: entry.creation_time.as_millis(),
        last_modification_time_ms: entry.last_modification_time.as_millis(),
        last_access_time_ms: entry.last_access_time.as_millis(),
        location_changed_ms: entry.location_changed.as_millis(),
        expires: entry.expires,
        expiry_time_ms: entry.expiry_time.as_millis(),
        usage_count: entry.usage_count,
        attachments: entry
            .binaries
            .iter()
            .map(|binary| EntryAttachmentInformation {
                name: binary.name.clone(),
                size: binary.data.len() as u64,
                is_protected: binary.is_protected,
            })
            .collect(),
    }
}
