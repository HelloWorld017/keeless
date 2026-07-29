use std::collections::HashSet;
use std::sync::Arc;

use indexmap::IndexMap;
use zeroize::{Zeroize, Zeroizing};

use crate::crypto::memory_protection::{MemoryField, MemoryProtectionContext, MemoryUnlockSession};
use crate::model::core::date::DateInstant;
use crate::model::core::node::NodeId;
use crate::model::core::security::ProtectedString;
use crate::model::db::composite_key::CompositeKey;
use crate::model::entry::{
    memory_field, EntryBinary, EntryField, EntryFieldId, EntryFields, StandardField,
};
use crate::model::exception::{DatabaseError, DatabaseResult};
use crate::model::meta::icon::{IconImage, IconImageStandard};
use uuid::Uuid;

use super::Database;

/// One field in the complete desired field list for an entry update.
#[derive(Clone, PartialEq, Eq)]
pub struct EntryFieldUpdate {
    /// Runtime ID in the original entry, or `None` for a new custom field.
    pub field_id: Option<EntryFieldId>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconUpdate {
    pub standard_id: u32,
    pub custom_uuid: Option<Uuid>,
}

/// Complete desired update for entry properties stored outside string fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryPropertiesUpdate {
    pub override_url: String,
    pub tags: Vec<String>,
    pub expires: bool,
    pub expiry_time_ms: Option<i64>,
    pub icon: Option<IconUpdate>,
}

/// A fully validated entry replacement ready for an infallible database commit.
pub struct PreparedEntryUpdate {
    entry_id: NodeId,
    updated: crate::model::entry::Entry,
    memory_protection_context: Option<Arc<MemoryProtectionContext>>,
}

impl Database {
    /// Atomically replace the complete ordered field list and optionally update entry properties.
    pub fn update_entry(
        &mut self,
        composite_key: &CompositeKey,
        entry_id: &NodeId,
        fields: &[EntryFieldUpdate],
        properties: Option<&EntryPropertiesUpdate>,
    ) -> DatabaseResult<bool> {
        let new_custom_field_ids = fields
            .iter()
            .filter(|field| field.field_id.is_none())
            .map(|_| Uuid::new_v4())
            .collect::<Vec<_>>();
        self.update_entry_at(
            composite_key,
            entry_id,
            fields,
            properties,
            &new_custom_field_ids,
            DateInstant::now(),
        )
    }

    /// Validate, prepare, and commit an entry update with deterministic IDs and timestamp.
    pub fn update_entry_at(
        &mut self,
        composite_key: &CompositeKey,
        entry_id: &NodeId,
        fields: &[EntryFieldUpdate],
        properties: Option<&EntryPropertiesUpdate>,
        new_custom_field_ids: &[Uuid],
        last_modification_time: DateInstant,
    ) -> DatabaseResult<bool> {
        let Some(prepared) = self.prepare_entry_update(
            composite_key,
            entry_id,
            fields,
            properties,
            &[],
            &[],
            new_custom_field_ids,
            last_modification_time,
        )?
        else {
            return Ok(false);
        };
        self.commit_entry_update(prepared);
        Ok(true)
    }

    /// Build an entry update without mutating the database.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_entry_update(
        &self,
        composite_key: &CompositeKey,
        entry_id: &NodeId,
        fields: &[EntryFieldUpdate],
        properties: Option<&EntryPropertiesUpdate>,
        attachments: &[EntryBinary],
        removed_attachment_indices: &[u64],
        new_custom_field_ids: &[Uuid],
        last_modification_time: DateInstant,
    ) -> DatabaseResult<Option<PreparedEntryUpdate>> {
        let original = self
            .entries
            .get(entry_id)
            .ok_or_else(|| DatabaseError::InvalidFormat("entry does not exist".into()))?;
        let mut source_ids = HashSet::new();
        let mut standard_seen = HashSet::new();
        for field in fields {
            match field.field_id {
                Some(id) if source_ids.insert(id) => {
                    if !original.fields.0.contains_key(&id) {
                        return Err(DatabaseError::InvalidFormat(
                            "entry field ID is invalid".into(),
                        ));
                    }
                    match id {
                        EntryFieldId::Standard(standard) => {
                            if field.name != standard.name() {
                                return Err(DatabaseError::InvalidFormat(
                                    "standard entry field was renamed".into(),
                                ));
                            }
                            standard_seen.insert(standard);
                        }
                        EntryFieldId::Custom(_)
                            if StandardField::from_name(&field.name).is_some() =>
                        {
                            return Err(DatabaseError::InvalidFormat(
                                "standard entry field was duplicated".into(),
                            ));
                        }
                        EntryFieldId::Custom(_) => {}
                    }
                }
                Some(_) => {
                    return Err(DatabaseError::InvalidFormat(
                        "entry field ID is invalid or duplicated".into(),
                    ));
                }
                None if field.value.is_none() => {
                    return Err(DatabaseError::InvalidFormat(
                        "new entry fields require a value".into(),
                    ));
                }
                None if StandardField::from_name(&field.name).is_some() => {
                    return Err(DatabaseError::InvalidFormat(
                        "standard entry field was duplicated".into(),
                    ));
                }
                None => {}
            }
        }
        if !StandardField::ALL
            .into_iter()
            .all(|field| standard_seen.contains(&field))
        {
            return Err(DatabaseError::InvalidFormat(
                "all standard entry fields are required".into(),
            ));
        }
        let expected_new_ids = fields
            .iter()
            .filter(|field| field.field_id.is_none())
            .count();
        let unique_new_ids = new_custom_field_ids.iter().copied().collect::<HashSet<_>>();
        if new_custom_field_ids.len() != expected_new_ids
            || unique_new_ids.len() != new_custom_field_ids.len()
            || new_custom_field_ids
                .iter()
                .any(|id| original.fields.0.contains_key(&EntryFieldId::Custom(*id)))
        {
            return Err(DatabaseError::InvalidFormat(
                "new custom entry field IDs are invalid or duplicated".into(),
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
        let requested_ids = fields
            .iter()
            .filter_map(|field| field.field_id)
            .collect::<Vec<_>>();
        let original_ids = original.fields.0.keys().copied().collect::<Vec<_>>();
        let mut changed = requested_ids != original_ids
            || fields.iter().any(|field| field.field_id.is_none())
            || !attachments.is_empty()
            || !removed_attachment_indices.is_empty();
        let removal_count = removed_attachment_indices.len();
        let removed_attachment_indices = removed_attachment_indices
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        if removed_attachment_indices.len() != removal_count
            || removed_attachment_indices
                .iter()
                .any(|index| *index >= original.binaries.len() as u64)
        {
            return Err(DatabaseError::InvalidFormat(
                "attachment index is invalid or duplicated".into(),
            ));
        }
        let mut unlock = MemoryUnlockSession::new(composite_key);
        let mut plaintexts = Vec::with_capacity(fields.len());
        for requested in fields {
            let Some(id) = requested.field_id else {
                plaintexts.push(None);
                continue;
            };
            let source = original.fields.0.get(&id).expect("validated source field");
            let source_memory = memory_field(id, &source.name);
            if requested.value.is_none() && !source.value.is_protected() {
                return Err(DatabaseError::InvalidFormat(
                    "only protected entry fields can preserve a hidden value".into(),
                ));
            }
            let plaintext =
                source
                    .value
                    .with_plaintext(&mut unlock, original.id, &source_memory, |value| {
                        Ok(Zeroizing::new(value.to_string()))
                    })?;
            let target_memory = memory_field(id, &requested.name);
            changed |= requested
                .value
                .as_deref()
                .is_some_and(|value| value != *plaintext)
                || source.value.is_protected() != requested.is_protected
                || source_memory != target_memory;
            plaintexts.push(Some(plaintext));
        }

        let mut updated = original.clone();
        let mut visible_fields = IndexMap::with_capacity(fields.len());
        let mut new_ids = new_custom_field_ids.iter().copied();
        unlock.with_root(&context, |root| {
            for (requested, plaintext) in fields.iter().zip(&plaintexts) {
                let id = requested.field_id.unwrap_or_else(|| {
                    EntryFieldId::Custom(new_ids.next().expect("new field IDs validated"))
                });
                let target_memory = memory_field(id, &requested.name);
                let field = if let Some(source_id) = requested.field_id {
                    let source = original
                        .fields
                        .0
                        .get(&source_id)
                        .expect("validated source field");
                    let source_memory = memory_field(source_id, &source.name);
                    let mut target = if requested.value.is_none()
                        && source_memory == target_memory
                        && requested.is_protected
                    {
                        source.value.clone()
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
                            &target_memory,
                            value,
                            requested.is_protected,
                        )?;
                        target
                    };
                    EntryField {
                        name: requested.name.clone(),
                        value: std::mem::take(&mut target),
                        xml_extensions: source.xml_extensions.clone(),
                    }
                } else {
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
                        &target_memory,
                        value,
                        requested.is_protected,
                    )?;
                    EntryField::new(requested.name.clone(), target)
                };
                visible_fields.insert(id, field);
            }
            Ok(())
        })?;

        updated.fields = EntryFields(visible_fields);

        if let Some(properties) = properties {
            changed |= updated.override_url != properties.override_url
                || updated.tags != properties.tags
                || updated.expires != properties.expires
                || updated.expiry_time
                    != properties
                        .expiry_time_ms
                        .map(DateInstant::EpochMillis)
                        .unwrap_or_else(DateInstant::never)
                || properties.icon.is_some_and(|icon| {
                    updated.icon != IconImage::Standard(IconImageStandard::new(icon.standard_id))
                        || updated.custom_icon_uuid != icon.custom_uuid
                });
            updated.override_url.clone_from(&properties.override_url);
            updated.tags.clone_from(&properties.tags);
            updated.expires = properties.expires;
            updated.expiry_time = properties
                .expiry_time_ms
                .map(DateInstant::EpochMillis)
                .unwrap_or_else(DateInstant::never);
            if let Some(icon) = properties.icon {
                updated.icon = IconImage::Standard(IconImageStandard::new(icon.standard_id));
                updated.custom_icon_uuid = icon.custom_uuid;
            }
        }

        updated.binaries = original
            .binaries
            .iter()
            .enumerate()
            .filter(|(index, _)| !removed_attachment_indices.contains(&(*index as u64)))
            .map(|(_, attachment)| attachment.clone())
            .collect();
        updated.binaries.extend_from_slice(attachments);

        if !changed {
            return Ok(None);
        }
        updated.last_modification_time = last_modification_time;
        Ok(Some(PreparedEntryUpdate {
            entry_id: *entry_id,
            updated,
            memory_protection_context: self.memory_protection_context.is_none().then_some(context),
        }))
    }

    /// Commit a prepared update. Preparation guarantees this path cannot fail.
    pub fn commit_entry_update(&mut self, prepared: PreparedEntryUpdate) {
        if let Some(context) = prepared.memory_protection_context {
            self.memory_protection_context = Some(context);
        }
        let current = self
            .entries
            .get_mut(&prepared.entry_id)
            .expect("prepared entry still exists");
        let mut snapshot = std::mem::replace(current, prepared.updated);
        snapshot.history.clear();
        snapshot.xml_extensions.history.clear();
        current.history.push(snapshot);
        if current.history.len() > 10 {
            current.history.remove(0);
        }
        self.mark_modified();
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
