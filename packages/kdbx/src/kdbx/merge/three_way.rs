use super::*;

impl DatabaseMerger {
    /// Three-way merge: merge source into target using base as common ancestor.
    ///
    /// If only one side changed, that change is applied. If both sides changed,
    /// the configured merge strategy resolves the conflict.
    pub fn merge_three_way(
        &self,
        target: &mut Database,
        source: &Database,
        base: &Database,
    ) -> MergeResult {
        let mut result = MergeResult::default();

        self.merge_groups_three_way(target, source, base, &mut result);

        for (id, source_entry) in &source.entries {
            match (base.entries.get(id), target.entries.get(id)) {
                (None, None) => {
                    add_entry_from(target, source, source_entry.clone(), *id);
                    result.entries_added += 1;
                }
                (None, Some(target_entry)) => {
                    if entry_differs(target_entry, source_entry) {
                        let resolution = self.resolve_entry_conflict(
                            target,
                            source,
                            source_entry,
                            id,
                            &mut result,
                        );
                        result.conflicts.push(MergeConflict {
                            node_id: *id,
                            conflict_type: ConflictType::EntryModified,
                            field: None,
                            resolution,
                        });
                    }
                }
                (Some(base_entry), None) => {
                    if entry_content_differs(source_entry, base_entry) {
                        let deletion_time = deleted_time(target, id).unwrap_or(0);
                        let take_source = match self.strategy {
                            MergeStrategy::Overwrite => true,
                            MergeStrategy::NewestWins => {
                                source_entry.last_modified() > deletion_time
                            }
                            MergeStrategy::KeepExisting | MergeStrategy::KeepBoth => false,
                        };
                        let resolution = if take_source {
                            add_entry_from(target, source, source_entry.clone(), *id);
                            clear_deleted(target, id);
                            result.entries_added += 1;
                            ConflictResolution::TookIncoming
                        } else if self.strategy == MergeStrategy::KeepBoth {
                            let mut duplicate = source_entry.clone();
                            duplicate.id = NodeId::new_uuid();
                            add_entry_from(target, source, duplicate, *id);
                            result.entries_added += 1;
                            ConflictResolution::Duplicated
                        } else {
                            ConflictResolution::KeptExisting
                        };
                        result.conflicts.push(MergeConflict {
                            node_id: *id,
                            conflict_type: ConflictType::EntryDeleteVsModify,
                            field: None,
                            resolution,
                        });
                    }
                }
                (Some(base_entry), Some(target_entry)) => {
                    let source_modified = entry_content_differs(source_entry, base_entry);
                    if source_modified {
                        let target_entry = target_entry.clone();
                        let merged = self.merge_entry_fields(
                            &target_entry,
                            source_entry,
                            base_entry,
                        );

                        if merged.entry != target_entry {
                            *target.entries.get_mut(id).expect("entry exists") = merged.entry;
                            result.entries_modified += 1;
                        }

                        if let Some(mut duplicate) = merged.duplicate {
                            duplicate.id = NodeId::new_uuid();
                            add_entry_from(target, source, duplicate, *id);
                            result.entries_added += 1;
                        }

                        let resolution = self.field_conflict_resolution();
                        result.conflicts.extend(merged.conflicts.into_iter().map(|field| {
                            MergeConflict {
                                node_id: *id,
                                conflict_type: ConflictType::EntryModified,
                                field: Some(field),
                                resolution: resolution.clone(),
                            }
                        }));
                    }
                }
            }
        }

        let ids_to_check: Vec<NodeId> = target.entries.keys().copied().collect();
        for id in &ids_to_check {
            if let (Some(base_entry), None) = (base.entries.get(id), source.entries.get(id)) {
                let target_entry = &target.entries[id];
                let target_modified = entry_content_differs(target_entry, base_entry);
                if target_modified {
                    let deletion_time = deleted_time(source, id).unwrap_or(0);
                    let delete = match self.strategy {
                        MergeStrategy::Overwrite => true,
                        MergeStrategy::NewestWins => deletion_time > target_entry.last_modified(),
                        MergeStrategy::KeepExisting | MergeStrategy::KeepBoth => false,
                    };
                    if delete {
                        remove_entry(target, id);
                        result.entries_deleted += 1;
                    }
                    result.conflicts.push(MergeConflict {
                        node_id: *id,
                        conflict_type: ConflictType::EntryDeleteVsModify,
                        field: None,
                        resolution: if delete {
                            ConflictResolution::TookIncoming
                        } else {
                            ConflictResolution::KeptExisting
                        },
                    });
                } else {
                    remove_entry(target, id);
                    result.entries_deleted += 1;
                }
            }
        }

        merge_deleted_objects(target, source);

        if merge_changed(&result) {
            target.mark_modified();
        }

        result
    }

    fn merge_entry_fields(
        &self,
        target: &Entry,
        source: &Entry,
        base: &Entry,
    ) -> EntryMergeOutcome {
        let source_is_newer = source.last_modified() > target.last_modified();
        let mut entry = target.clone();
        let mut incoming = source.clone();
        let mut conflicts = Vec::new();
        let mut source_applied = false;
        let mut target_applied = false;

        macro_rules! merge_value {
            ($field:expr, $base:expr, $target:expr, $source:expr, |$primary:ident, $secondary:ident| $assign:block) => {{
                let resolved = resolve_field(
                    &$base,
                    &$target,
                    &$source,
                    self.strategy,
                    source_is_newer,
                );
                let ResolvedField {
                    primary: $primary,
                    secondary: $secondary,
                    conflict,
                    source_applied: applied_source,
                    target_applied: applied_target,
                } = resolved;
                $assign
                source_applied |= applied_source;
                target_applied |= applied_target;
                if conflict {
                    conflicts.push($field);
                }
            }};
        }

        merge_value!(
            ConflictField::Title,
            (base.title.clone(), base.title_is_protected),
            (target.title.clone(), target.title_is_protected),
            (source.title.clone(), source.title_is_protected),
            |primary, secondary| {
                (entry.title, entry.title_is_protected) = primary;
                (incoming.title, incoming.title_is_protected) = secondary;
            }
        );
        merge_value!(
            ConflictField::Username,
            base.username.clone(),
            target.username.clone(),
            source.username.clone(),
            |primary, secondary| {
                entry.username = primary;
                incoming.username = secondary;
            }
        );
        merge_value!(
            ConflictField::Password,
            base.password.clone(),
            target.password.clone(),
            source.password.clone(),
            |primary, secondary| {
                entry.password = primary;
                incoming.password = secondary;
            }
        );
        merge_value!(
            ConflictField::Url,
            (base.url.clone(), base.url_is_protected),
            (target.url.clone(), target.url_is_protected),
            (source.url.clone(), source.url_is_protected),
            |primary, secondary| {
                (entry.url, entry.url_is_protected) = primary;
                (incoming.url, incoming.url_is_protected) = secondary;
            }
        );
        merge_value!(
            ConflictField::Notes,
            base.notes.clone(),
            target.notes.clone(),
            source.notes.clone(),
            |primary, secondary| {
                entry.notes = primary;
                incoming.notes = secondary;
            }
        );
        merge_value!(
            ConflictField::Icon,
            (base.icon.clone(), base.custom_icon_uuid),
            (target.icon.clone(), target.custom_icon_uuid),
            (source.icon.clone(), source.custom_icon_uuid),
            |primary, secondary| {
                (entry.icon, entry.custom_icon_uuid) = primary;
                (incoming.icon, incoming.custom_icon_uuid) = secondary;
            }
        );
        merge_value!(
            ConflictField::BackgroundColor,
            base.background_color.clone(),
            target.background_color.clone(),
            source.background_color.clone(),
            |primary, secondary| {
                entry.background_color = primary;
                incoming.background_color = secondary;
            }
        );
        merge_value!(
            ConflictField::ForegroundColor,
            base.foreground_color.clone(),
            target.foreground_color.clone(),
            source.foreground_color.clone(),
            |primary, secondary| {
                entry.foreground_color = primary;
                incoming.foreground_color = secondary;
            }
        );
        merge_value!(
            ConflictField::OverrideUrl,
            base.override_url.clone(),
            target.override_url.clone(),
            source.override_url.clone(),
            |primary, secondary| {
                entry.override_url = primary;
                incoming.override_url = secondary;
            }
        );
        merge_value!(
            ConflictField::Tags,
            base.tags.clone(),
            target.tags.clone(),
            source.tags.clone(),
            |primary, secondary| {
                entry.tags = primary;
                incoming.tags = secondary;
            }
        );
        merge_value!(
            ConflictField::Expiry,
            (base.expiry_time, base.expires),
            (target.expiry_time, target.expires),
            (source.expiry_time, source.expires),
            |primary, secondary| {
                (entry.expiry_time, entry.expires) = primary;
                (incoming.expiry_time, incoming.expires) = secondary;
            }
        );
        merge_value!(
            ConflictField::AutoType,
            base.auto_type.clone(),
            target.auto_type.clone(),
            source.auto_type.clone(),
            |primary, secondary| {
                entry.auto_type = primary;
                incoming.auto_type = secondary;
            }
        );
        merge_value!(
            ConflictField::IsTemplate,
            base.is_template,
            target.is_template,
            source.is_template,
            |primary, secondary| {
                entry.is_template = primary;
                incoming.is_template = secondary;
            }
        );

        for name in entry_field_names(base, target, source) {
            let base_field = find_entry_field(base, &name).cloned();
            let target_field = find_entry_field(target, &name).cloned();
            let source_field = find_entry_field(source, &name).cloned();
            let resolved = resolve_field(
                &base_field,
                &target_field,
                &source_field,
                self.strategy,
                source_is_newer,
            );
            set_entry_field(&mut entry.custom_fields, &name, resolved.primary);
            set_entry_field(&mut incoming.custom_fields, &name, resolved.secondary);
            source_applied |= resolved.source_applied;
            target_applied |= resolved.target_applied;
            if resolved.conflict {
                conflicts.push(ConflictField::CustomField(name));
            }
        }

        for name in entry_binary_names(base, target, source) {
            let base_binary = find_entry_binary(base, &name).cloned();
            let target_binary = find_entry_binary(target, &name).cloned();
            let source_binary = find_entry_binary(source, &name).cloned();
            let resolved = resolve_field(
                &base_binary,
                &target_binary,
                &source_binary,
                self.strategy,
                source_is_newer,
            );
            set_entry_binary(&mut entry.binaries, &name, resolved.primary);
            set_entry_binary(&mut incoming.binaries, &name, resolved.secondary);
            source_applied |= resolved.source_applied;
            target_applied |= resolved.target_applied;
            if resolved.conflict {
                conflicts.push(ConflictField::Binary(name));
            }
        }

        for key in custom_data_keys(base, target, source) {
            let base_item = find_custom_data_item(&base.custom_data, &key).cloned();
            let target_item = find_custom_data_item(&target.custom_data, &key).cloned();
            let source_item = find_custom_data_item(&source.custom_data, &key).cloned();
            let base_value = base_item.as_ref().map(|item| item.value.clone());
            let target_value = target_item.as_ref().map(|item| item.value.clone());
            let source_value = source_item.as_ref().map(|item| item.value.clone());
            let resolved = resolve_field(
                &base_value,
                &target_value,
                &source_value,
                self.strategy,
                source_is_newer,
            );
            let primary = select_custom_data_item(
                &resolved.primary,
                target_item.as_ref(),
                source_item.as_ref(),
            );
            let secondary = select_custom_data_item(
                &resolved.secondary,
                source_item.as_ref(),
                target_item.as_ref(),
            );
            set_custom_data_item(&mut entry.custom_data, &key, primary);
            set_custom_data_item(&mut incoming.custom_data, &key, secondary);
            source_applied |= resolved.source_applied;
            target_applied |= resolved.target_applied;
            if resolved.conflict {
                conflicts.push(ConflictField::CustomData(key));
            }
        }

        merge_entry_metadata(&mut entry, target, source, source_applied);
        merge_entry_metadata(&mut incoming, source, target, target_applied);

        let duplicate =
            (self.strategy == MergeStrategy::KeepBoth && !conflicts.is_empty()).then_some(incoming);
        EntryMergeOutcome {
            entry,
            duplicate,
            conflicts,
        }
    }

    fn field_conflict_resolution(&self) -> ConflictResolution {
        match self.strategy {
            MergeStrategy::KeepExisting => ConflictResolution::KeptExisting,
            MergeStrategy::Overwrite => ConflictResolution::TookIncoming,
            MergeStrategy::KeepBoth => ConflictResolution::Duplicated,
            MergeStrategy::NewestWins => ConflictResolution::NewestUsed,
        }
    }

    fn merge_groups_three_way(
        &self,
        target: &mut Database,
        source: &Database,
        base: &Database,
        result: &mut MergeResult,
    ) {
        if target.root_group_id.is_none() {
            target.root_group_id = source.root_group_id;
        }
        for (id, source_group) in &source.groups {
            match (base.groups.get(id), target.groups.get(id)) {
                (None, None) => {
                    if source.root_group_id != Some(*id) || target.root_group_id == Some(*id) {
                        let mut group = source_group.clone();
                        group.child_group_ids.clear();
                        group.child_entry_ids.clear();
                        target.groups.insert(*id, group);
                        result.groups_added += 1;
                    }
                }
                (None, Some(target_group)) => {
                    if group_content_differs(target_group, source_group) {
                        result.conflicts.push(MergeConflict {
                            node_id: *id,
                            conflict_type: ConflictType::GroupModified,
                            field: None,
                            resolution: ConflictResolution::KeptExisting,
                        });
                    }
                }
                (Some(base_group), Some(target_group)) => {
                    let source_modified = group_content_differs(source_group, base_group);
                    let target_modified = group_content_differs(target_group, base_group);
                    if source_modified && !target_modified {
                        copy_group_content(
                            target.groups.get_mut(id).expect("group exists"),
                            source_group,
                        );
                        result.groups_modified += 1;
                    } else if source_modified
                        && target_modified
                        && group_content_differs(target_group, source_group)
                    {
                        let take_source = matches!(self.strategy, MergeStrategy::Overwrite)
                            || (self.strategy == MergeStrategy::NewestWins
                                && source_group.last_modified() > target_group.last_modified());
                        if take_source {
                            copy_group_content(
                                target.groups.get_mut(id).expect("group exists"),
                                source_group,
                            );
                            result.groups_modified += 1;
                        }
                        result.conflicts.push(MergeConflict {
                            node_id: *id,
                            conflict_type: ConflictType::GroupModified,
                            field: None,
                            resolution: if take_source {
                                ConflictResolution::TookIncoming
                            } else {
                                ConflictResolution::KeptExisting
                            },
                        });
                    }
                }
                (Some(base_group), None) => {
                    if group_content_differs(source_group, base_group) {
                        let deletion_time = deleted_time(target, id).unwrap_or(0);
                        let take_source = self.strategy == MergeStrategy::Overwrite
                            || (self.strategy == MergeStrategy::NewestWins
                                && source_group.last_modified() > deletion_time);
                        if take_source {
                            let mut group = source_group.clone();
                            group.child_group_ids.clear();
                            group.child_entry_ids.clear();
                            target.groups.insert(*id, group);
                            attach_group_from(target, source, *id);
                            clear_deleted(target, id);
                            result.groups_added += 1;
                        }
                        result.conflicts.push(MergeConflict {
                            node_id: *id,
                            conflict_type: ConflictType::GroupModified,
                            field: None,
                            resolution: if take_source {
                                ConflictResolution::TookIncoming
                            } else {
                                ConflictResolution::KeptExisting
                            },
                        });
                    }
                }
            }
        }

        let added: Vec<NodeId> = source
            .groups
            .keys()
            .filter(|id| !base.groups.contains_key(id) && target.groups.contains_key(id))
            .copied()
            .collect();
        for id in added {
            attach_group_from(target, source, id);
        }

        let common: Vec<NodeId> = source
            .groups
            .keys()
            .filter(|id| base.groups.contains_key(id) && target.groups.contains_key(id))
            .copied()
            .collect();
        for id in common {
            if Some(id) == target.root_group_id {
                continue;
            }
            let base_parent = group_parent(base, &id);
            let source_parent = group_parent(source, &id);
            let target_parent = group_parent(target, &id);
            let source_changed = source_parent != base_parent;
            let target_changed = target_parent != base_parent;
            if source_changed && (!target_changed || source_parent == target_parent) {
                move_group_from(target, source, id);
            } else if source_changed && target_changed && source_parent != target_parent {
                let take_source = matches!(self.strategy, MergeStrategy::Overwrite)
                    || (self.strategy == MergeStrategy::NewestWins
                        && source.groups[&id].location_changed.as_millis().unwrap_or(0)
                            > target.groups[&id].location_changed.as_millis().unwrap_or(0));
                if take_source {
                    move_group_from(target, source, id);
                }
                result.conflicts.push(MergeConflict {
                    node_id: id,
                    conflict_type: ConflictType::GroupModified,
                    field: None,
                    resolution: if take_source {
                        ConflictResolution::TookIncoming
                    } else {
                        ConflictResolution::KeptExisting
                    },
                });
            }
        }

        let deleted_groups: Vec<NodeId> = target
            .groups
            .keys()
            .filter(|id| base.groups.contains_key(id) && !source.groups.contains_key(id))
            .copied()
            .collect();
        for id in deleted_groups {
            if !target.groups.contains_key(&id) || target.root_group_id == Some(id) {
                continue;
            }
            let target_modified = group_content_differs(&target.groups[&id], &base.groups[&id])
                || group_parent(target, &id) != group_parent(base, &id);
            let deletion_time = deleted_time(source, &id).unwrap_or(0);
            let delete = !target_modified
                || self.strategy == MergeStrategy::Overwrite
                || (self.strategy == MergeStrategy::NewestWins
                    && deletion_time > target.groups[&id].last_modified());
            if delete {
                let (groups, entries) = remove_group(target, &id);
                result.groups_deleted += groups;
                result.entries_deleted += entries;
            } else {
                result.conflicts.push(MergeConflict {
                    node_id: id,
                    conflict_type: ConflictType::GroupModified,
                    field: None,
                    resolution: ConflictResolution::KeptExisting,
                });
            }
        }
    }
}

struct EntryMergeOutcome {
    entry: Entry,
    duplicate: Option<Entry>,
    conflicts: Vec<ConflictField>,
}

struct ResolvedField<T> {
    primary: T,
    secondary: T,
    conflict: bool,
    source_applied: bool,
    target_applied: bool,
}

fn resolve_field<T: Clone + Eq>(
    base: &T,
    target: &T,
    source: &T,
    strategy: MergeStrategy,
    source_is_newer: bool,
) -> ResolvedField<T> {
    if target == source {
        return ResolvedField {
            primary: target.clone(),
            secondary: source.clone(),
            conflict: false,
            source_applied: false,
            target_applied: false,
        };
    }
    if target == base {
        return ResolvedField {
            primary: source.clone(),
            secondary: source.clone(),
            conflict: false,
            source_applied: true,
            target_applied: false,
        };
    }
    if source == base {
        return ResolvedField {
            primary: target.clone(),
            secondary: target.clone(),
            conflict: false,
            source_applied: false,
            target_applied: true,
        };
    }

    let take_source = strategy == MergeStrategy::Overwrite
        || (strategy == MergeStrategy::NewestWins && source_is_newer);
    ResolvedField {
        primary: if take_source {
            source.clone()
        } else {
            target.clone()
        },
        secondary: source.clone(),
        conflict: true,
        source_applied: take_source,
        target_applied: false,
    }
}

fn entry_content_differs(a: &Entry, b: &Entry) -> bool {
    a.title != b.title
        || a.title_is_protected != b.title_is_protected
        || a.username != b.username
        || a.password != b.password
        || a.url != b.url
        || a.url_is_protected != b.url_is_protected
        || a.notes != b.notes
        || a.icon != b.icon
        || a.custom_icon_uuid != b.custom_icon_uuid
        || a.background_color != b.background_color
        || a.foreground_color != b.foreground_color
        || a.override_url != b.override_url
        || a.tags != b.tags
        || a.custom_fields != b.custom_fields
        || a.binaries != b.binaries
        || a.expiry_time != b.expiry_time
        || a.expires != b.expires
        || a.auto_type != b.auto_type
        || !custom_data_values_equal(&a.custom_data, &b.custom_data)
        || a.is_template != b.is_template
}

fn custom_data_values_equal(a: &CustomData, b: &CustomData) -> bool {
    a.len() == b.len()
        && a
            .iter()
            .all(|(key, item)| b.get(key) == Some(item.value.as_str()))
}

fn entry_field_names(base: &Entry, target: &Entry, source: &Entry) -> Vec<String> {
    let mut names = Vec::new();
    for field in base
        .custom_fields
        .iter()
        .chain(&target.custom_fields)
        .chain(&source.custom_fields)
    {
        push_unique(&mut names, &field.name);
    }
    names
}

fn find_entry_field<'a>(entry: &'a Entry, name: &str) -> Option<&'a EntryField> {
    entry.custom_fields.iter().find(|field| field.name == name)
}

fn set_entry_field(fields: &mut Vec<EntryField>, name: &str, field: Option<EntryField>) {
    let position = fields.iter().position(|item| item.name == name);
    fields.retain(|item| item.name != name);
    if let Some(field) = field {
        fields.insert(position.unwrap_or(fields.len()).min(fields.len()), field);
    }
}

fn entry_binary_names(base: &Entry, target: &Entry, source: &Entry) -> Vec<String> {
    let mut names = Vec::new();
    for binary in base
        .binaries
        .iter()
        .chain(&target.binaries)
        .chain(&source.binaries)
    {
        push_unique(&mut names, &binary.name);
    }
    names
}

fn find_entry_binary<'a>(entry: &'a Entry, name: &str) -> Option<&'a EntryBinary> {
    entry.binaries.iter().find(|binary| binary.name == name)
}

fn set_entry_binary(binaries: &mut Vec<EntryBinary>, name: &str, binary: Option<EntryBinary>) {
    let position = binaries.iter().position(|item| item.name == name);
    binaries.retain(|item| item.name != name);
    if let Some(binary) = binary {
        binaries.insert(position.unwrap_or(binaries.len()).min(binaries.len()), binary);
    }
}

fn custom_data_keys(base: &Entry, target: &Entry, source: &Entry) -> Vec<String> {
    let mut keys = Vec::new();
    for (key, _) in base
        .custom_data
        .iter()
        .chain(target.custom_data.iter())
        .chain(source.custom_data.iter())
    {
        push_unique(&mut keys, key);
    }
    keys
}

fn find_custom_data_item<'a>(data: &'a CustomData, key: &str) -> Option<&'a CustomDataItem> {
    data.iter()
        .find_map(|(item_key, item)| (item_key == key).then_some(item))
}

fn select_custom_data_item(
    value: &Option<String>,
    preferred: Option<&CustomDataItem>,
    alternate: Option<&CustomDataItem>,
) -> Option<CustomDataItem> {
    let value = value.as_deref()?;
    match (
        preferred.filter(|item| item.value == value),
        alternate.filter(|item| item.value == value),
    ) {
        (Some(preferred), Some(alternate)) => {
            if alternate.last_modification_time > preferred.last_modification_time {
                Some(alternate.clone())
            } else {
                Some(preferred.clone())
            }
        }
        (Some(item), None) | (None, Some(item)) => Some(item.clone()),
        (None, None) => None,
    }
}

fn set_custom_data_item(data: &mut CustomData, key: &str, item: Option<CustomDataItem>) {
    if let Some(item) = item {
        data.insert(key.to_string(), item);
    } else {
        data.remove(key);
    }
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|item| item == value) {
        values.push(value.to_string());
    }
}

fn merge_entry_metadata(entry: &mut Entry, original: &Entry, other: &Entry, other_applied: bool) {
    entry.id = original.id;
    entry.creation_time = original.creation_time;
    entry.location_changed = original.location_changed;
    entry.last_modification_time = if other_applied
        && other.last_modification_time.as_millis()
            > original.last_modification_time.as_millis()
    {
        other.last_modification_time
    } else {
        original.last_modification_time
    };
    if other.last_access_time.as_millis() > original.last_access_time.as_millis() {
        entry.last_access_time = other.last_access_time;
    } else {
        entry.last_access_time = original.last_access_time;
    }
    entry.usage_count = original.usage_count.max(other.usage_count);
    entry.history = original.history.clone();
    for historical_entry in &other.history {
        if !entry.history.contains(historical_entry) {
            entry.history.push(historical_entry.clone());
        }
    }
}
