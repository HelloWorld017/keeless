use super::super::*;

impl DatabaseMerger<'_> {
    pub(super) fn merge_entries_three_way(
        &self,
        target: &mut Database,
        source: &Database,
        base: &Database,
        retained_groups: &[NodeId],
        result: &mut MergeResult,
    ) {
        for (id, source_entry) in &source.entries {
            match (base.entries.get(id), target.entries.get(id)) {
                (None, None) => {
                    add_entry_from(target, source, source_entry.clone(), *id);
                    result.entries_added += 1;
                }
                (None, Some(target_entry)) => {
                    let target_entry = target_entry.clone();
                    if self.entry_differs(target, &target_entry, source, source_entry) {
                        let resolution = self.resolve_entry_conflict(
                            target,
                            source,
                            source_entry,
                            id,
                            None,
                            result,
                        );
                        result.conflicts.push(MergeConflict {
                            node_id: *id,
                            conflict_type: ConflictType::EntryModified,
                            resolution,
                        });
                    } else {
                        let merged = merge_entry_histories(
                            target_entry.clone(),
                            &[&target_entry, source_entry],
                        );
                        if merged != target_entry {
                            target.entries.insert(*id, merged);
                            result.entries_modified += 1;
                        }
                    }
                }
                (Some(base_entry), None) => {
                    if self.entry_differs(source, source_entry, base, base_entry)
                        || self.entry_history_differs(source, source_entry, base, base_entry)
                    {
                        let deletion_time = deleted_time(target, id).unwrap_or(0);
                        let take_source = match self.strategy {
                            MergeStrategy::Overwrite => true,
                            MergeStrategy::NewestWins => {
                                source_entry.last_modified() > deletion_time
                            }
                            MergeStrategy::KeepExisting | MergeStrategy::KeepBoth => false,
                        };
                        let resolution = if take_source {
                            let entry = merge_entry_histories(
                                source_entry.clone(),
                                &[source_entry, base_entry],
                            );
                            add_entry_from(target, source, entry, *id);
                            clear_deleted(target, id);
                            result.entries_added += 1;
                            ConflictResolution::TookIncoming
                        } else if self.strategy == MergeStrategy::KeepBoth {
                            let mut duplicate = merge_entry_histories(
                                source_entry.clone(),
                                &[source_entry, base_entry],
                            );
                            if self.rebind_entry(&mut duplicate, NodeId::new_uuid()) {
                                add_entry_from(target, source, duplicate, *id);
                                result.entries_added += 1;
                            }
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
                    let target_entry = target_entry.clone();
                    let source_modified =
                        self.entry_differs(source, source_entry, base, base_entry);
                    let target_modified =
                        self.entry_differs(target, &target_entry, base, base_entry);

                    match (source_modified, target_modified) {
                        (true, false) => {
                            let merged = merge_entry_histories(
                                source_entry.clone(),
                                &[&target_entry, source_entry, base_entry],
                            );
                            if merged != target_entry {
                                target.entries.insert(*id, merged);
                                result.entries_modified += 1;
                            }
                        }
                        (false, true) => {
                            let merged = merge_entry_histories(
                                target_entry.clone(),
                                &[&target_entry, source_entry, base_entry],
                            );
                            if merged != target_entry {
                                target.entries.insert(*id, merged);
                                result.entries_modified += 1;
                            }
                        }
                        (true, true) => {
                            if self.entry_differs(target, &target_entry, source, source_entry) {
                                let resolution = self.resolve_entry_conflict(
                                    target,
                                    source,
                                    source_entry,
                                    id,
                                    Some(base_entry),
                                    result,
                                );
                                result.conflicts.push(MergeConflict {
                                    node_id: *id,
                                    conflict_type: ConflictType::EntryModified,
                                    resolution,
                                });
                            } else {
                                let merged = merge_entry_histories(
                                    target_entry.clone(),
                                    &[&target_entry, source_entry, base_entry],
                                );
                                if merged != target_entry {
                                    target.entries.insert(*id, merged);
                                    result.entries_modified += 1;
                                }
                            }
                        }
                        (false, false) => {
                            let merged = merge_entry_histories(
                                target_entry.clone(),
                                &[&target_entry, source_entry, base_entry],
                            );
                            if merged != target_entry {
                                target.entries.insert(*id, merged);
                                result.entries_modified += 1;
                            }
                        }
                    }
                }
            }
        }

        let common: Vec<NodeId> = source
            .entries
            .keys()
            .filter(|id| base.entries.contains_key(id) && target.entries.contains_key(id))
            .copied()
            .collect();
        for id in common {
            let base_parent = base.find_parent_group_of_entry(&id);
            let source_parent = source.find_parent_group_of_entry(&id);
            let target_parent = target.find_parent_group_of_entry(&id);
            let source_changed = source_parent != base_parent;
            let target_changed = target_parent != base_parent;
            if source_changed && (!target_changed || source_parent == target_parent) {
                if move_entry_from(target, source, id) {
                    result.entries_modified += 1;
                }
            } else if source_changed && target_changed && source_parent != target_parent {
                let take_source = matches!(self.strategy, MergeStrategy::Overwrite)
                    || (self.strategy == MergeStrategy::NewestWins
                        && source.entries[&id]
                            .location_changed
                            .as_millis()
                            .unwrap_or(0)
                            > target.entries[&id]
                                .location_changed
                                .as_millis()
                                .unwrap_or(0));
                if take_source && move_entry_from(target, source, id) {
                    result.entries_modified += 1;
                }
                result.conflicts.push(MergeConflict {
                    node_id: id,
                    conflict_type: ConflictType::EntryModified,
                    resolution: if take_source {
                        ConflictResolution::TookIncoming
                    } else {
                        ConflictResolution::KeptExisting
                    },
                });
            }
        }

        let ids_to_check: Vec<NodeId> = target.entries.keys().copied().collect();
        for id in &ids_to_check {
            if let (Some(base_entry), None) = (base.entries.get(id), source.entries.get(id)) {
                let retained_with_group =
                    target.find_parent_group_of_entry(id).is_some_and(|parent| {
                        retained_groups
                            .iter()
                            .any(|root| group_contains(target, root, &parent))
                    });
                if retained_with_group {
                    continue;
                }
                let target_entry = &target.entries[id];
                let target_modified = self.entry_differs(target, target_entry, base, base_entry)
                    || self.entry_history_differs(target, target_entry, base, base_entry)
                    || target.find_parent_group_of_entry(id) != base.find_parent_group_of_entry(id);
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
    }

    fn resolve_entry_conflict(
        &self,
        target: &mut Database,
        source: &Database,
        source_entry: &Entry,
        id: &NodeId,
        base_entry: Option<&Entry>,
        result: &mut MergeResult,
    ) -> ConflictResolution {
        let target_entry = target.entries.get(id).expect("entry exists").clone();
        let mut common_history = vec![&target_entry, source_entry];
        if let Some(base_entry) = base_entry {
            common_history.push(base_entry);
        }

        match self.strategy {
            MergeStrategy::KeepExisting => {
                let merged = merge_entry_histories(target_entry.clone(), &common_history);
                if merged != target_entry {
                    target.entries.insert(*id, merged);
                    result.entries_modified += 1;
                }
                ConflictResolution::KeptExisting
            }
            MergeStrategy::Overwrite => {
                let merged = merge_entry_histories(source_entry.clone(), &common_history);
                if merged != target_entry {
                    target.entries.insert(*id, merged);
                    result.entries_modified += 1;
                }
                ConflictResolution::TookIncoming
            }
            MergeStrategy::KeepBoth => {
                let mut target_history = vec![&target_entry];
                if let Some(base_entry) = base_entry {
                    target_history.push(base_entry);
                }
                let merged_target = merge_entry_histories(target_entry.clone(), &target_history);
                if merged_target != target_entry {
                    target.entries.insert(*id, merged_target);
                    result.entries_modified += 1;
                }

                let duplicate = source_entry.clone();
                let mut source_history = vec![source_entry];
                if let Some(base_entry) = base_entry {
                    source_history.push(base_entry);
                }
                let mut duplicate = merge_entry_histories(duplicate, &source_history);
                if self.rebind_entry(&mut duplicate, NodeId::new_uuid()) {
                    add_entry_from(target, source, duplicate, *id);
                    result.entries_added += 1;
                }
                ConflictResolution::Duplicated
            }
            MergeStrategy::NewestWins => {
                let winner = if source_entry.last_modified() > target_entry.last_modified() {
                    source_entry.clone()
                } else {
                    target_entry.clone()
                };
                let merged = merge_entry_histories(winner, &common_history);
                if merged != target_entry {
                    target.entries.insert(*id, merged);
                    result.entries_modified += 1;
                }
                ConflictResolution::NewestUsed
            }
        }
    }
}
