use std::collections::HashSet;

use keeless_kdbx::{Database, Entry, EntryFieldSelector, IconImage, NodeId};
use keeless_schema::{
    DatabaseNodeId, EntryAttachmentInformation, EntryDetailResult, EntryFieldInformation,
    EntrySummary, GroupHierarchyItem, GroupHierarchyResult, IconReference,
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

fn node_id(id: NodeId) -> DatabaseNodeId {
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
        name: (!entry.title_is_protected).then(|| entry.title.clone()),
        name_is_protected: entry.title_is_protected,
        url: (!entry.url_is_protected).then(|| entry.url.clone()),
        url_is_protected: entry.url_is_protected,
        icon: icon_reference(&entry.icon, entry.custom_icon_uuid),
        tags: entry.tags.clone(),
    }
}

pub(super) fn all_entry_summaries(database: &Database) -> Vec<EntrySummary> {
    fn visit_group(
        database: &Database,
        group_id: &NodeId,
        visited_groups: &mut HashSet<NodeId>,
        visited_entries: &mut HashSet<NodeId>,
        entries: &mut Vec<EntrySummary>,
    ) {
        if !visited_groups.insert(*group_id) {
            return;
        }
        let Some(group) = database.get_group(group_id) else {
            return;
        };

        for entry_id in &group.child_entry_ids {
            if visited_entries.insert(*entry_id)
                && let Some(entry) = database.get_entry(entry_id)
            {
                entries.push(entry_summary(entry));
            }
        }
        for child_group_id in &group.child_group_ids {
            visit_group(
                database,
                child_group_id,
                visited_groups,
                visited_entries,
                entries,
            );
        }
    }

    let mut entries = Vec::with_capacity(database.entries.len());
    let mut visited_groups = HashSet::new();
    let mut visited_entries = HashSet::new();
    if let Some(root_group_id) = &database.root_group_id {
        visit_group(
            database,
            root_group_id,
            &mut visited_groups,
            &mut visited_entries,
            &mut entries,
        );
    }

    let mut remaining = database
        .entries
        .keys()
        .filter(|id| !visited_entries.contains(id))
        .collect::<Vec<_>>();
    remaining.sort_by_key(|id| node_id_sort_key(id));
    entries.extend(
        remaining
            .into_iter()
            .filter_map(|id| database.get_entry(id))
            .map(entry_summary),
    );
    entries
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

fn standard_field(
    entry: &Entry,
    name: &str,
    selector: EntryFieldSelector,
    is_protected: bool,
) -> EntryFieldInformation {
    let value = if is_protected {
        None
    } else {
        entry.with_unsealed_field(&selector, str::to_owned)
    };
    EntryFieldInformation {
        name: name.to_string(),
        value,
        is_protected,
    }
}

pub(super) fn entry_detail(entry: &Entry) -> EntryDetailResult {
    let mut fields = vec![
        standard_field(
            entry,
            "Title",
            EntryFieldSelector::Title,
            entry.title_is_protected,
        ),
        standard_field(
            entry,
            "UserName",
            EntryFieldSelector::UserName,
            entry.username.is_protected(),
        ),
        standard_field(
            entry,
            "Password",
            EntryFieldSelector::Password,
            entry.password.is_protected(),
        ),
        standard_field(
            entry,
            "URL",
            EntryFieldSelector::Url,
            entry.url_is_protected,
        ),
        standard_field(
            entry,
            "Notes",
            EntryFieldSelector::Notes,
            entry.notes.is_protected(),
        ),
    ];
    fields.extend(entry.custom_fields.iter().map(|field| {
        let is_protected = field.is_protected || field.value.is_protected();
        EntryFieldInformation {
            name: field.name.clone(),
            value: (!is_protected).then(|| field.value.as_str().to_string()),
            is_protected,
        }
    }));

    EntryDetailResult {
        id: node_id(entry.id),
        icon: icon_reference(&entry.icon, entry.custom_icon_uuid),
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
