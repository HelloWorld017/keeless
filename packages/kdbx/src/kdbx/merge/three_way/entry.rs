use super::super::*;

impl DatabaseMerger {
    pub(super) fn merge_entries_three_way(
        &self,
        target: &mut Database,
        source: &Database,
        base: &Database,
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
                    if entry_differs(&target_entry, source_entry) {
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
                    if entry_differs(source_entry, base_entry)
                        || entry_history_differs(source_entry, base_entry)
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
                            let mut duplicate = source_entry.clone();
                            duplicate.id = NodeId::new_uuid();
                            let duplicate =
                                merge_entry_histories(duplicate, &[source_entry, base_entry]);
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
                    let target_entry = target_entry.clone();
                    let source_modified = entry_differs(source_entry, base_entry);
                    let target_modified = entry_differs(&target_entry, base_entry);

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
                            if entry_differs(&target_entry, source_entry) {
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

        let ids_to_check: Vec<NodeId> = target.entries.keys().copied().collect();
        for id in &ids_to_check {
            if let (Some(base_entry), None) = (base.entries.get(id), source.entries.get(id)) {
                let target_entry = &target.entries[id];
                let target_modified = entry_differs(target_entry, base_entry)
                    || entry_history_differs(target_entry, base_entry);
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

                let mut duplicate = source_entry.clone();
                duplicate.id = NodeId::new_uuid();
                let mut source_history = vec![source_entry];
                if let Some(base_entry) = base_entry {
                    source_history.push(base_entry);
                }
                let duplicate = merge_entry_histories(duplicate, &source_history);
                add_entry_from(target, source, duplicate, *id);
                result.entries_added += 1;
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
