use std::collections::HashSet;
use std::sync::Arc;

use indexmap::IndexMap;
use zeroize::{Zeroize, Zeroizing};

use crate::crypto::memory_protection::{MemoryField, MemoryProtectionContext, MemoryUnlockSession};
use crate::model::core::date::DateInstant;
use crate::model::core::node::NodeId;
use crate::model::core::security::ProtectedString;
use crate::model::db::composite_key::CompositeKey;
use crate::model::entry::{memory_field, EntryField, EntryFieldId, EntryFields, StandardField};
use crate::model::exception::{DatabaseError, DatabaseResult};
use crate::model::meta::ETM_PREFIX;

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

/// Complete desired update for entry properties stored outside string fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryPropertiesUpdate {
    pub override_url: String,
    pub tags: Vec<String>,
    pub expires: bool,
    pub expiry_time_ms: Option<i64>,
}

impl Database {
    /// Atomically replace the complete ordered field list.
    /// Returns `false` when the requested representation is semantically unchanged.
    pub fn update_entry_fields(
        &mut self,
        composite_key: &CompositeKey,
        entry_id: &NodeId,
        fields: &[EntryFieldUpdate],
    ) -> DatabaseResult<bool> {
        self.update_entry(composite_key, entry_id, fields, None)
    }

    /// Atomically replace all client-visible fields and optionally update entry properties.
    /// Reserved ETM fields are retained unchanged and cannot be submitted by clients.
    pub fn update_entry(
        &mut self,
        composite_key: &CompositeKey,
        entry_id: &NodeId,
        fields: &[EntryFieldUpdate],
        properties: Option<&EntryPropertiesUpdate>,
    ) -> DatabaseResult<bool> {
        let original = self
            .entries
            .get(entry_id)
            .ok_or_else(|| DatabaseError::InvalidFormat("entry does not exist".into()))?;
        let mut source_ids = HashSet::new();
        let mut standard_seen = HashSet::new();
        for field in fields {
            if field.name.starts_with(ETM_PREFIX) {
                return Err(DatabaseError::InvalidFormat(
                    "reserved entry fields cannot be updated".into(),
                ));
            }
            match field.field_id {
                Some(id) if source_ids.insert(id) => {
                    let Some(source) = original.fields.0.get(&id) else {
                        return Err(DatabaseError::InvalidFormat(
                            "entry field ID is invalid".into(),
                        ));
                    };
                    if source.name.starts_with(ETM_PREFIX) {
                        return Err(DatabaseError::InvalidFormat(
                            "reserved entry fields cannot be updated".into(),
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
        let original_ids = original
            .fields
            .0
            .iter()
            .filter(|(_, field)| !field.name.starts_with(ETM_PREFIX))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        let mut changed =
            requested_ids != original_ids || fields.iter().any(|field| field.field_id.is_none());
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
        unlock.with_root(&context, |root| {
            for (requested, plaintext) in fields.iter().zip(&plaintexts) {
                let id = requested
                    .field_id
                    .unwrap_or_else(|| EntryFieldId::Custom(uuid::Uuid::new_v4()));
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

        let mut visible_fields = visible_fields.into_iter();
        let mut updated_fields = IndexMap::with_capacity(original.fields.0.len() + fields.len());
        for (id, field) in &original.fields.0 {
            if field.name.starts_with(ETM_PREFIX) {
                updated_fields.insert(*id, field.clone());
            } else if let Some((id, field)) = visible_fields.next() {
                updated_fields.insert(id, field);
            }
        }
        updated_fields.extend(visible_fields);
        updated.fields = EntryFields(updated_fields);

        if let Some(properties) = properties {
            changed |= updated.override_url != properties.override_url
                || updated.tags != properties.tags
                || updated.expires != properties.expires
                || updated.expiry_time
                    != properties
                        .expiry_time_ms
                        .map(DateInstant::EpochMillis)
                        .unwrap_or_else(DateInstant::never);
            updated.override_url.clone_from(&properties.override_url);
            updated.tags.clone_from(&properties.tags);
            updated.expires = properties.expires;
            updated.expiry_time = properties
                .expiry_time_ms
                .map(DateInstant::EpochMillis)
                .unwrap_or_else(DateInstant::never);
        }

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
