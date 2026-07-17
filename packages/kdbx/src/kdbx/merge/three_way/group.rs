use super::super::*;

impl DatabaseMerger {
    pub(super) fn merge_groups_three_way(
        &self,
        target: &mut Database,
        source: &Database,
        base: &Database,
        result: &mut MergeResult,
    ) -> Vec<NodeId> {
        let mut retained_groups = Vec::new();
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
                if move_group_from(target, source, id) {
                    result.groups_modified += 1;
                }
            } else if source_changed && target_changed && source_parent != target_parent {
                let take_source = matches!(self.strategy, MergeStrategy::Overwrite)
                    || (self.strategy == MergeStrategy::NewestWins
                        && source.groups[&id].location_changed.as_millis().unwrap_or(0)
                            > target.groups[&id].location_changed.as_millis().unwrap_or(0));
                if take_source && move_group_from(target, source, id) {
                    result.groups_modified += 1;
                }
                result.conflicts.push(MergeConflict {
                    node_id: id,
                    conflict_type: ConflictType::GroupModified,
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
            .filter(|id| {
                base.groups.contains_key(id)
                    && !source.groups.contains_key(id)
                    && group_parent(base, id)
                        .map_or(true, |parent| source.groups.contains_key(&parent))
            })
            .copied()
            .collect();
        for id in deleted_groups {
            if !target.groups.contains_key(&id) || target.root_group_id == Some(id) {
                continue;
            }
            let target_modified = group_tree_modified(target, base, id);
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
                retained_groups.push(id);
                result.conflicts.push(MergeConflict {
                    node_id: id,
                    conflict_type: ConflictType::GroupModified,
                    resolution: ConflictResolution::KeptExisting,
                });
            }
        }
        retained_groups
    }
}

fn group_tree_modified(target: &Database, base: &Database, root_id: NodeId) -> bool {
    let mut stack = vec![root_id];
    while let Some(id) = stack.pop() {
        let Some(target_group) = target.groups.get(&id) else {
            continue;
        };
        let Some(base_group) = base.groups.get(&id) else {
            return true;
        };
        if group_content_differs(target_group, base_group)
            || group_parent(target, &id) != group_parent(base, &id)
        {
            return true;
        }
        for entry_id in &target_group.child_entry_ids {
            let Some(target_entry) = target.entries.get(entry_id) else {
                continue;
            };
            let Some(base_entry) = base.entries.get(entry_id) else {
                return true;
            };
            if entry_differs(target_entry, base_entry)
                || entry_history_differs(target_entry, base_entry)
                || target.find_parent_group_of_entry(entry_id)
                    != base.find_parent_group_of_entry(entry_id)
            {
                return true;
            }
        }
        stack.extend(target_group.child_group_ids.iter().copied());
    }
    false
}
