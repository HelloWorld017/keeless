//! Security types.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::crypto::memory_protection::{
    EncryptedValue, MemoryField, MemoryProtectionContext, MemoryUnlockSession,
};
use crate::model::core::node::NodeId;
use crate::model::exception::{DatabaseError, DatabaseResult};

/// A string whose KDBX serialization uses inner-stream protection.
///
/// Values parsed by the low-level XML API are temporarily unsealed. The full
/// database open path seals them with credential-derived authenticated
/// encryption before returning the database to the caller.
#[derive(Clone, PartialEq, Eq)]
pub struct ProtectedString {
    state: ProtectedStringState,
}

#[derive(Clone, PartialEq, Eq)]
enum ProtectedStringState {
    Plain(String),
    Unsealed(String),
    Sealed(EncryptedValue),
}

impl std::fmt::Debug for ProtectedString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_protected() {
            f.write_str("Protected([REDACTED])")
        } else {
            f.write_str("Plain([REDACTED])")
        }
    }
}

impl ProtectedString {
    /// Create a new empty protected string.
    pub fn new() -> Self {
        Self::new_protected("")
    }

    /// Create a plain, non-protected string.
    pub fn new_plain(value: &str) -> Self {
        Self {
            state: ProtectedStringState::Plain(value.to_string()),
        }
    }

    /// Create a protected value awaiting attachment to a credential-scoped database.
    ///
    /// Prefer `Database::set_entry_field` in application code; it encrypts the
    /// value immediately. This constructor remains for low-level import APIs.
    pub fn new_protected(value: &str) -> Self {
        Self {
            state: ProtectedStringState::Unsealed(value.to_string()),
        }
    }

    pub fn is_protected(&self) -> bool {
        !matches!(self.state, ProtectedStringState::Plain(_))
    }

    pub fn is_memory_protected(&self) -> bool {
        matches!(self.state, ProtectedStringState::Sealed(_))
    }

    /// Access plaintext that has not yet been memory-protected.
    ///
    /// Sealed values deliberately cannot return an escaping `&str`; use the
    /// credential-scoped database APIs for values returned by `open_database`.
    pub fn as_str(&self) -> &str {
        match &self.state {
            ProtectedStringState::Plain(value) | ProtectedStringState::Unsealed(value) => value,
            ProtectedStringState::Sealed(_) => {
                panic!("memory-protected value requires credential-scoped access")
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.state {
            ProtectedStringState::Plain(value) | ProtectedStringState::Unsealed(value) => {
                value.is_empty()
            }
            // XChaCha20-Poly1305 appends a 16-byte tag and does not pad.
            ProtectedStringState::Sealed(value) => value.ciphertext().len() == 16,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.as_str().as_bytes()
    }

    pub(crate) fn seal(
        &mut self,
        context: Arc<MemoryProtectionContext>,
        root: &[u8; 32],
        entry_id: NodeId,
        field: &MemoryField,
    ) -> DatabaseResult<()> {
        let ProtectedStringState::Unsealed(value) = &mut self.state else {
            return Ok(());
        };
        let encrypted = EncryptedValue::encrypt(context, root, entry_id, field, value.as_bytes())?;
        value.zeroize();
        self.state = ProtectedStringState::Sealed(encrypted);
        Ok(())
    }

    pub(crate) fn seal_as_protected(
        &mut self,
        context: Arc<MemoryProtectionContext>,
        root: &[u8; 32],
        entry_id: NodeId,
        field: &MemoryField,
    ) -> DatabaseResult<()> {
        if let ProtectedStringState::Plain(value) = &mut self.state {
            let mut plaintext = String::new();
            std::mem::swap(value, &mut plaintext);
            self.state = ProtectedStringState::Unsealed(plaintext);
        }
        self.seal(context, root, entry_id, field)
    }

    pub(crate) fn replace_sealed(
        &mut self,
        context: Arc<MemoryProtectionContext>,
        root: &[u8; 32],
        entry_id: NodeId,
        field: &MemoryField,
        value: &str,
    ) -> DatabaseResult<()> {
        self.state = ProtectedStringState::Sealed(EncryptedValue::encrypt(
            context,
            root,
            entry_id,
            field,
            value.as_bytes(),
        )?);
        Ok(())
    }

    pub(crate) fn replace_plain(&mut self, value: &str) {
        self.state = ProtectedStringState::Plain(value.to_string());
    }

    pub(crate) fn replace_unsealed(&mut self, value: &str) {
        self.state = ProtectedStringState::Unsealed(value.to_string());
    }

    pub(crate) fn with_plaintext<T>(
        &self,
        unlock: &mut MemoryUnlockSession<'_>,
        entry_id: NodeId,
        field: &MemoryField,
        use_value: impl FnOnce(&str) -> DatabaseResult<T>,
    ) -> DatabaseResult<T> {
        match &self.state {
            ProtectedStringState::Plain(value) | ProtectedStringState::Unsealed(value) => {
                use_value(value)
            }
            ProtectedStringState::Sealed(value) => unlock.with_root(&value.context, |root| {
                let plaintext = value.decrypt(root, entry_id, field)?;
                let text = std::str::from_utf8(plaintext.as_slice()).map_err(|err| {
                    DatabaseError::DecryptionError(format!(
                        "memory-protected string is not UTF-8: {err}"
                    ))
                })?;
                use_value(text)
            }),
        }
    }

    pub(crate) fn rebind(
        &mut self,
        unlock: &mut MemoryUnlockSession<'_>,
        old_entry_id: NodeId,
        new_entry_id: NodeId,
        field: &MemoryField,
    ) -> DatabaseResult<()> {
        let ProtectedStringState::Sealed(value) = &self.state else {
            return Ok(());
        };
        let context = value.context.clone();
        let encrypted = unlock.with_root(&context, |root| {
            let plaintext = value.decrypt(root, old_entry_id, field)?;
            EncryptedValue::encrypt(
                context.clone(),
                root,
                new_entry_id,
                field,
                plaintext.as_slice(),
            )
        })?;
        self.state = ProtectedStringState::Sealed(encrypted);
        Ok(())
    }
}

impl Default for ProtectedString {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ProtectedString {
    fn drop(&mut self) {
        match &mut self.state {
            ProtectedStringState::Plain(value) | ProtectedStringState::Unsealed(value) => {
                value.zeroize()
            }
            ProtectedStringState::Sealed(_) => {}
        }
    }
}

#[derive(Serialize, Deserialize)]
enum SerializableProtectedString {
    Plain(String),
    Protected(String),
}

impl Serialize for ProtectedString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match &self.state {
            ProtectedStringState::Plain(value) => {
                SerializableProtectedString::Plain(value.clone()).serialize(serializer)
            }
            ProtectedStringState::Unsealed(value) => {
                SerializableProtectedString::Protected(value.clone()).serialize(serializer)
            }
            ProtectedStringState::Sealed(_) => Err(serde::ser::Error::custom(
                "memory-protected values require explicit credential-scoped export",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for ProtectedString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(
            match SerializableProtectedString::deserialize(deserializer)? {
                SerializableProtectedString::Plain(value) => Self::new_plain(&value),
                SerializableProtectedString::Protected(value) => Self::new_protected(&value),
            },
        )
    }
}

/// Memory protection defaults stored in `Meta/MemoryProtection`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryProtectionConfig {
    pub protect_title: bool,
    pub protect_username: bool,
    pub protect_password: bool,
    pub protect_url: bool,
    pub protect_notes: bool,
    pub auto_enable_visual_hiding: bool,
}
