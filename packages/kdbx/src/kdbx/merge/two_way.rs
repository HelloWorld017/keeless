use super::*;

impl DatabaseMerger {
    /// Two-way merge: merge source database into target database.
    pub fn merge(&self, target: &mut Database, source: &Database) -> MergeResult {
        let mut result = MergeResult::default();

        self.merge_groups(target, source, &mut result);
        self.merge_entries(target, source, &mut result);

        for deleted in &source.deleted_objects {
            if source.entries.contains_key(&deleted.id) || source.groups.contains_key(&deleted.id) {
                continue;
            }
            if let Some(entry) = target.entries.get(&deleted.id) {
                if deleted.deletion_time >= entry.last_modified() {
                    remove_entry(target, &deleted.id);
                    result.entries_deleted += 1;
                } else {
                    result.conflicts.push(MergeConflict {
                        node_id: deleted.id,
                        conflict_type: ConflictType::EntryDeleteVsModify,
                        resolution: ConflictResolution::KeptExisting,
                    });
                }
            } else if target.groups.contains_key(&deleted.id)
                && target.root_group_id != Some(deleted.id)
            {
                let modified = target
                    .groups
                    .get(&deleted.id)
                    .map_or(0, Node::last_modified);
                if deleted.deletion_time >= modified {
                    let (groups, entries) = remove_group(target, &deleted.id);
                    result.groups_deleted += groups;
                    result.entries_deleted += entries;
                }
            }
        }

        let icons_changed = merge_custom_icons(target, source);
        let deleted_objects_changed = merge_deleted_objects(target, source);

        if merge_changed(&result) || icons_changed || deleted_objects_changed {
            target.mark_modified();
        }

        result
    }

    fn merge_groups(&self, target: &mut Database, source: &Database, result: &mut MergeResult) {
        if target.root_group_id.is_none() {
            target.root_group_id = source.root_group_id;
        }
        for (id, source_group) in &source.groups {
            if let Some(target_group) = target.groups.get_mut(id) {
                if source_group.last_modified() > target_group.last_modified() {
                    match self.strategy {
                        MergeStrategy::Overwrite | MergeStrategy::NewestWins => {
                            copy_group_content(target_group, source_group);
                            result.groups_modified += 1;
                        }
                        MergeStrategy::KeepExisting | MergeStrategy::KeepBoth => {}
                    }
                }
            } else if source.root_group_id != Some(*id) || target.root_group_id == Some(*id) {
                let mut group = source_group.clone();
                group.child_group_ids.clear();
                group.child_entry_ids.clear();
                target.groups.insert(*id, group);
                result.groups_added += 1;
            }
        }
        let added: Vec<NodeId> = source
            .groups
            .keys()
            .filter(|id| target.groups.contains_key(id) && source.root_group_id != Some(**id))
            .copied()
            .collect();
        for id in added {
            if group_parent(source, &id).is_some() && group_parent(target, &id).is_none() {
                attach_group_from(target, source, id);
            }
        }
    }

    fn merge_entries(&self, target: &mut Database, source: &Database, result: &mut MergeResult) {
        for (id, source_entry) in &source.entries {
            if let Some(target_entry) = target.entries.get_mut(id) {
                let original = target_entry.clone();
                let source_is_newer = source_entry.last_modified() > original.last_modified();

                if source_is_newer && self.strategy == MergeStrategy::KeepBoth {
                    let mut duplicate = source_entry.clone();
                    duplicate.id = NodeId::new_uuid();
                    let duplicate = merge_entry_histories(duplicate, &[source_entry]);
                    add_entry_from(target, source, duplicate, *id);
                    result.entries_added += 1;
                    continue;
                }

                let winner = if source_is_newer
                    && matches!(
                        self.strategy,
                        MergeStrategy::Overwrite | MergeStrategy::NewestWins
                    ) {
                    source_entry.clone()
                } else {
                    original.clone()
                };
                let merged = merge_entry_histories(winner, &[&original, source_entry]);
                if merged != original {
                    *target_entry = merged;
                    result.entries_modified += 1;
                }
            } else {
                add_entry_from(target, source, source_entry.clone(), *id);
                clear_deleted(target, id);
                result.entries_added += 1;
            }
        }
    }
}

fn merge_custom_icons(target: &mut Database, source: &Database) -> bool {
    let previous_count = target.custom_icons.len();
    for (uuid, icon) in &source.custom_icons {
        target
            .custom_icons
            .entry(*uuid)
            .or_insert_with(|| icon.clone());
    }
    target.custom_icons.len() != previous_count
}
