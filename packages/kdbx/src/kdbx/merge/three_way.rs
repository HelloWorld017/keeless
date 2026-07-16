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
                            resolution,
                        });
                    }
                }
                (Some(base_entry), None) => {
                    if entry_differs(source_entry, base_entry) {
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
                            resolution,
                        });
                    }
                }
                (Some(base_entry), Some(target_entry)) => {
                    let source_modified = entry_differs(source_entry, base_entry);
                    let target_modified = entry_differs(target_entry, base_entry);

                    match (source_modified, target_modified) {
                        (true, false) => {
                            *target.entries.get_mut(id).expect("entry exists") =
                                source_entry.clone();
                            result.entries_modified += 1;
                        }
                        (false, true) => {}
                        (true, true) => {
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
                                    resolution,
                                });
                            }
                        }
                        (false, false) => {}
                    }
                }
            }
        }

        let ids_to_check: Vec<NodeId> = target.entries.keys().copied().collect();
        for id in &ids_to_check {
            if let (Some(base_entry), None) = (base.entries.get(id), source.entries.get(id)) {
                let target_entry = &target.entries[id];
                let target_modified = entry_differs(target_entry, base_entry);
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
                    resolution: ConflictResolution::KeptExisting,
                });
            }
        }
    }
}
