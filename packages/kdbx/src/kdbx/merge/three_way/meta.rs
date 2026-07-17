use std::collections::HashSet;

use super::super::*;
use crate::model::meta::CustomData;

/// Applies source metadata only when target still matches the common ancestor.
/// Database-level fields do not carry reliable conflict timestamps, so target
/// wins when both sides changed the same value.
pub(super) fn merge_database_metadata_three_way(
    target: &mut Database,
    source: &Database,
    base: &Database,
) -> bool {
    let mut changed = false;
    changed |= merge_copy(&mut target.version, source.version, base.version);
    changed |= merge_copy(
        &mut target.file_version,
        source.file_version,
        base.file_version,
    );
    changed |= merge_copy(
        &mut target.encryption_algorithm,
        source.encryption_algorithm,
        base.encryption_algorithm,
    );
    changed |= merge_copy(
        &mut target.compression,
        source.compression,
        base.compression,
    );
    changed |= merge_clone(
        &mut target.kdf_parameters,
        &source.kdf_parameters,
        &base.kdf_parameters,
    );
    changed |= merge_clone(
        &mut target.public_custom_data,
        &source.public_custom_data,
        &base.public_custom_data,
    );
    changed |= merge_clone(
        &mut target.header_comment,
        &source.header_comment,
        &base.header_comment,
    );
    changed |= merge_clone(&mut target.name, &source.name, &base.name);
    changed |= merge_clone(
        &mut target.description,
        &source.description,
        &base.description,
    );
    changed |= merge_clone(
        &mut target.default_username,
        &source.default_username,
        &base.default_username,
    );
    changed |= merge_copy(
        &mut target.recycle_bin_uuid,
        source.recycle_bin_uuid,
        base.recycle_bin_uuid,
    );
    changed |= merge_copy(
        &mut target.entry_templates_uuid,
        source.entry_templates_uuid,
        base.entry_templates_uuid,
    );
    changed |= merge_clone(
        &mut target.memory_protection,
        &source.memory_protection,
        &base.memory_protection,
    );
    changed |= merge_custom_data_three_way(
        &mut target.custom_data,
        &source.custom_data,
        &base.custom_data,
    );
    changed |= merge_clone(
        &mut target.xml_extensions,
        &source.xml_extensions,
        &base.xml_extensions,
    );
    if target.contains_unsupported_xml == base.contains_unsupported_xml
        && source.contains_unsupported_xml != base.contains_unsupported_xml
    {
        target.contains_unsupported_xml = source.contains_unsupported_xml;
        changed = true;
    }
    changed
}

fn merge_custom_data_three_way(
    target: &mut CustomData,
    source: &CustomData,
    base: &CustomData,
) -> bool {
    let previous = target.clone();
    let keys: HashSet<_> = base
        .iter()
        .chain(source.iter())
        .chain(target.iter())
        .map(|(key, _)| key.clone())
        .collect();
    for key in keys {
        let base_value = base
            .iter()
            .find(|(name, _)| *name == &key)
            .map(|(_, item)| item);
        let source_value = source
            .iter()
            .find(|(name, _)| *name == &key)
            .map(|(_, item)| item);
        let target_value = target
            .iter()
            .find(|(name, _)| *name == &key)
            .map(|(_, item)| item);
        if source_value == base_value || target_value != base_value {
            continue;
        }
        match source_value {
            Some(item) => target.insert(key, item.clone()),
            None => target.remove(&key),
        }
    }
    merge_clone(
        &mut target.xml_extensions,
        &source.xml_extensions,
        &base.xml_extensions,
    );
    *target != previous
}

pub(super) fn merge_custom_icons_three_way(
    target: &mut Database,
    source: &Database,
    base: &Database,
) -> bool {
    let previous = target.custom_icons.clone();
    let ids: HashSet<_> = base
        .custom_icons
        .keys()
        .chain(source.custom_icons.keys())
        .chain(target.custom_icons.keys())
        .copied()
        .collect();
    for id in ids {
        let base_icon = base.custom_icons.get(&id);
        let source_icon = source.custom_icons.get(&id);
        let target_icon = target.custom_icons.get(&id);
        if source_icon == base_icon || target_icon != base_icon {
            continue;
        }
        match source_icon {
            Some(icon) => {
                target.custom_icons.insert(id, icon.clone());
            }
            None => {
                target.custom_icons.remove(&id);
            }
        }
    }
    target.custom_icons != previous
}

fn merge_copy<T: Copy + PartialEq>(target: &mut T, source: T, base: T) -> bool {
    if *target == base && source != base {
        *target = source;
        true
    } else {
        false
    }
}

fn merge_clone<T: Clone + PartialEq>(target: &mut T, source: &T, base: &T) -> bool {
    if target == base && source != base {
        *target = source.clone();
        true
    } else {
        false
    }
}
