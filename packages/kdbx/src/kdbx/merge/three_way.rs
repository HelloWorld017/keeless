mod entry;
mod group;
mod meta;

use super::*;

impl DatabaseMerger<'_> {
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

        let retained_groups = self.merge_groups_three_way(target, source, base, &mut result);
        self.merge_entries_three_way(target, source, base, &retained_groups, &mut result);

        let metadata_changed = meta::merge_database_metadata_three_way(target, source, base);
        let icons_changed = meta::merge_custom_icons_three_way(target, source, base);
        let deleted_objects_changed = merge_deleted_objects(target, source);

        if merge_changed(&result) || metadata_changed || icons_changed || deleted_objects_changed {
            target.mark_modified();
        }

        result
    }
}
