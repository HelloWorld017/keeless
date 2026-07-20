use std::collections::HashSet;
use std::sync::Arc;

use zeroize::{Zeroize, Zeroizing};

use crate::crypto::memory_protection::{MemoryField, MemoryProtectionContext, MemoryUnlockSession};
use crate::model::core::date::DateInstant;
use crate::model::core::node::NodeId;
use crate::model::core::security::ProtectedString;
use crate::model::db::composite_key::CompositeKey;
use crate::model::entry::{Entry, EntryField};
use crate::model::exception::{DatabaseError, DatabaseResult};

use super::Database;

/// One field in the complete desired field list for an entry update.
#[derive(Clone, PartialEq, Eq)]
pub struct EntryFieldUpdate {
    /// Absolute index in the original entry, or `None` for a new custom field.
    pub field_index: Option<usize>,
    pub name: String,
    /// `None` preserves an existing protected value without exposing it.
    pub value: Option<String>,
    pub is_protected: bool,
}

impl Drop for EntryFieldUpdate {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

impl Database {
    /// Atomically replace all standard fields and the complete ordered custom-field list.
    /// Returns `false` when the requested representation is semantically unchanged.
    pub fn update_entry_fields(
        &mut self,
        composite_key: &CompositeKey,
        entry_id: &NodeId,
        fields: &[EntryFieldUpdate],
    ) -> DatabaseResult<bool> {
        const STANDARD_NAMES: [&str; 5] = ["Title", "UserName", "Password", "URL", "Notes"];

        let original = self
            .entries
            .get(entry_id)
            .ok_or_else(|| DatabaseError::InvalidFormat("entry does not exist".into()))?;
        let source_count = 5 + original.custom_fields.len();
        let mut source_indices = HashSet::new();
        let mut standard_seen = [false; 5];
        for field in fields {
            match field.field_index {
                Some(index) if index < source_count && source_indices.insert(index) => {
                    if index < 5 {
                        if field.name != STANDARD_NAMES[index] {
                            return Err(DatabaseError::InvalidFormat(
                                "standard entry field was renamed".into(),
                            ));
                        }
                        standard_seen[index] = true;
                    } else if STANDARD_NAMES.contains(&field.name.as_str()) {
                        return Err(DatabaseError::InvalidFormat(
                            "standard entry field was duplicated".into(),
                        ));
                    }
                }
                Some(_) => {
                    return Err(DatabaseError::InvalidFormat(
                        "entry field source index is invalid or duplicated".into(),
                    ));
                }
                None if field.value.is_none() => {
                    return Err(DatabaseError::InvalidFormat(
                        "new entry fields require a value".into(),
                    ));
                }
                None if STANDARD_NAMES.contains(&field.name.as_str()) => {
                    return Err(DatabaseError::InvalidFormat(
                        "standard entry field was duplicated".into(),
                    ));
                }
                None => {}
            }
        }
        if !standard_seen.into_iter().all(|seen| seen) {
            return Err(DatabaseError::InvalidFormat(
                "all standard entry fields are required".into(),
            ));
        }

        let context = self
            .memory_protection_context
            .clone()
            .map(Ok)
            .unwrap_or_else(|| {
                self.create_memory_context(composite_key)
                    .map(|(context, _)| context)
            })?;
        let mut updated = original.clone();
        updated.custom_fields.clear();
        updated.xml_extensions.custom_strings.clear();
        let retained_custom_count = fields
            .iter()
            .filter(|field| field.field_index.is_some_and(|index| index >= 5))
            .count();
        let mut changed = retained_custom_count != original.custom_fields.len();
        let mut custom_position = 5;
        let mut unlock = MemoryUnlockSession::new(composite_key);
        let mut plaintexts = Vec::with_capacity(fields.len());
        for requested in fields {
            let Some(index) = requested.field_index else {
                plaintexts.push(None);
                changed = true;
                continue;
            };
            if index >= 5 {
                changed |= index != custom_position;
                custom_position += 1;
            }
            let (source_value, source_field) = entry_value_at(&original, index);
            if requested.value.is_none() && !source_value.is_protected() {
                return Err(DatabaseError::InvalidFormat(
                    "only protected entry fields can preserve a hidden value".into(),
                ));
            }
            let plaintext =
                source_value.with_plaintext(&mut unlock, original.id, &source_field, |value| {
                    Ok(Zeroizing::new(value.to_string()))
                })?;
            let target_field = entry_memory_field(index, &requested.name);
            changed |= requested
                .value
                .as_deref()
                .is_some_and(|value| value != *plaintext)
                || source_value.is_protected() != requested.is_protected
                || source_field != target_field;
            plaintexts.push(Some(plaintext));
        }

        unlock.with_root(&context, |root| {
            for (requested, plaintext) in fields.iter().zip(&plaintexts) {
                let (source_value, source_field) = match requested.field_index {
                    Some(index) => entry_value_at(&original, index),
                    None => {
                        let value = requested
                            .value
                            .as_deref()
                            .expect("new field value validated");
                        let mut target = ProtectedString::new_plain(value);
                        replace_entry_value(
                            &mut target,
                            context.clone(),
                            root,
                            original.id,
                            &MemoryField::Custom(requested.name.clone()),
                            value,
                            requested.is_protected,
                        )?;
                        updated.custom_fields.push(EntryField {
                            name: requested.name.clone(),
                            value: target,
                        });
                        updated.xml_extensions.custom_strings.push(Vec::new());
                        changed = true;
                        continue;
                    }
                };
                let index = requested.field_index.expect("existing source");
                let target_field = entry_memory_field(index, &requested.name);

                let mut target = if requested.value.is_none()
                    && source_field == target_field
                    && requested.is_protected
                {
                    source_value.clone()
                } else {
                    let value = requested
                        .value
                        .as_deref()
                        .unwrap_or_else(|| plaintext.as_ref().expect("existing plaintext"));
                    let mut target = ProtectedString::new_plain(value);
                    replace_entry_value(
                        &mut target,
                        context.clone(),
                        root,
                        original.id,
                        &target_field,
                        value,
                        requested.is_protected,
                    )?;
                    target
                };

                match requested.field_index.expect("existing source") {
                    0 => updated.title = target,
                    1 => updated.username = target,
                    2 => updated.password = target,
                    3 => updated.url = target,
                    4 => updated.notes = target,
                    _ => {
                        updated.custom_fields.push(EntryField {
                            name: requested.name.clone(),
                            value: std::mem::take(&mut target),
                        });
                        updated.xml_extensions.custom_strings.push(
                            original
                                .xml_extensions
                                .custom_strings
                                .get(index - 5)
                                .cloned()
                                .unwrap_or_default(),
                        );
                    }
                }
            }
            Ok(())
        })?;

        if !changed {
            return Ok(false);
        }
        updated.last_modification_time = DateInstant::now();
        if self.memory_protection_context.is_none() {
            self.memory_protection_context = Some(context);
        }
        let current = self
            .entries
            .get_mut(entry_id)
            .expect("entry existence validated");
        let mut snapshot = std::mem::replace(current, updated);
        snapshot.history.clear();
        snapshot.xml_extensions.history.clear();
        current.history.push(snapshot);
        if current.history.len() > 10 {
            current.history.remove(0);
        }
        self.mark_modified();
        Ok(true)
    }
}

fn replace_entry_value(
    target: &mut ProtectedString,
    context: Arc<MemoryProtectionContext>,
    root: &[u8; 32],
    entry_id: NodeId,
    field: &MemoryField,
    value: &str,
    protected: bool,
) -> DatabaseResult<()> {
    if protected {
        target.replace_sealed(context, root, entry_id, field, value)
    } else {
        target.replace_plain(value);
        Ok(())
    }
}

fn entry_value_at(entry: &Entry, index: usize) -> (&ProtectedString, MemoryField) {
    match index {
        0 => (&entry.title, MemoryField::Title),
        1 => (&entry.username, MemoryField::UserName),
        2 => (&entry.password, MemoryField::Password),
        3 => (&entry.url, MemoryField::Url),
        4 => (&entry.notes, MemoryField::Notes),
        _ => {
            let field = &entry.custom_fields[index - 5];
            (&field.value, MemoryField::Custom(field.name.clone()))
        }
    }
}

fn entry_memory_field(index: usize, name: &str) -> MemoryField {
    match index {
        0 => MemoryField::Title,
        1 => MemoryField::UserName,
        2 => MemoryField::Password,
        3 => MemoryField::Url,
        4 => MemoryField::Notes,
        _ => MemoryField::Custom(name.to_string()),
    }
}
