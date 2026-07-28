use keeless_kdbx::{
    CompositeKey, CtapRegistrationRequest, Database, EntryFieldId, PasskeyAuthenticator,
    PasskeyCredential, StandardField,
};
use keeless_schema::{OperationSuccess, RegisterPasskeyArgs, RegisterPasskeyResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::extensions::passkey::PasskeyExtension;
use crate::features::passkeys;
use crate::model::node_id;
use crate::{CoreError, KeelessCore, PasswordInputMode, Result};

use super::{Mutation as JournalMutation, add_entry, mutate, update_entry};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Mutation {
    pub(super) parent: keeless_kdbx::NodeId,
    pub(super) entry: update_entry::Mutation,
}

fn prepare(
    database: &Database,
    mutation: &Mutation,
    key: &CompositeKey,
) -> Result<keeless_kdbx::PreparedEntryUpdate> {
    let add = add_entry::Mutation {
        parent: mutation.parent,
        id: mutation.entry.id,
        timestamp_ms: mutation.entry.timestamp_ms,
    };
    let mut preview = database.clone();
    add_entry::apply(&mut preview, &add)?;
    update_entry::prepare(&preview, &mutation.entry, key)?.ok_or(CoreError::InvalidJournal)
}

fn commit(
    database: &mut Database,
    mutation: &Mutation,
    prepared: keeless_kdbx::PreparedEntryUpdate,
) {
    let add = add_entry::Mutation {
        parent: mutation.parent,
        id: mutation.entry.id,
        timestamp_ms: mutation.entry.timestamp_ms,
    };
    add_entry::apply(database, &add).expect("registration preflight validated entry creation");
    database.commit_entry_update(prepared);
    database
        .get_entry_mut(&mutation.entry.id)
        .expect("registration preflight created entry")
        .clear_history();
}

pub(super) fn apply(
    database: &mut Database,
    mutation: &Mutation,
    key: &CompositeKey,
) -> Result<()> {
    let prepared = prepare(database, mutation, key)?;
    commit(database, mutation, prepared);
    Ok(())
}

pub(crate) async fn run(
    core: &mut KeelessCore,
    args: RegisterPasskeyArgs,
) -> Result<RegisterPasskeyResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let client_data_hash = passkeys::decode(&args.client_data_hash)?;
    let user_handle = passkeys::decode(&args.user_handle)?;
    let exclude_credential_ids = args
        .exclude_credential_ids
        .iter()
        .map(|value| passkeys::decode(value))
        .collect::<Result<Vec<_>>>()?;
    let parent_group_id = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database()
        .root_group_id
        .ok_or(CoreError::GroupNotFound)?;

    if !exclude_credential_ids.is_empty()
        && !passkeys::visible_credentials(core, Some(&args.rp_id), &exclude_credential_ids)?
            .is_empty()
    {
        return Err(CoreError::PasskeyExcluded);
    }
    let result = PasskeyAuthenticator::create_ctap(&CtapRegistrationRequest {
        client_data_hash: &client_data_hash,
        rp_id: &args.rp_id,
        user_handle: &user_handle,
        username: &args.user_name,
        algorithms: &args.algorithms,
        existing_credentials: &[],
        exclude_credential_ids: &exclude_credential_ids
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>(),
        user_verification: passkeys::user_verification(args.user_verified),
    })?;
    let title = args
        .rp_name
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or(result.credential.rp_id());
    let fields = entry_fields(title, &args.user_name, &result.credential)?;
    let key = passkeys::unlock(core, PasswordInputMode::Save).await?;
    let entry_id = keeless_kdbx::NodeId::new_uuid();
    let new_custom_field_ids = fields
        .iter()
        .filter(|field| field.field_id.is_none())
        .map(|_| Uuid::new_v4())
        .collect();
    let payload = Mutation {
        parent: parent_group_id,
        entry: update_entry::Mutation {
            id: entry_id,
            fields,
            properties: None,
            new_custom_field_ids,
            timestamp_ms: core.clock.now_millis(),
        },
    };
    let prepared = prepare(
        core.handle
            .as_ref()
            .ok_or(CoreError::DatabaseLocked)?
            .database(),
        &payload,
        &key,
    )?;
    let mutation = JournalMutation::RegisterPasskey(payload.clone());
    mutate(core, &mutation, move |database| {
        commit(database, &payload, prepared);
    })
    .await?;

    core.extensions
        .get_mut::<PasskeyExtension>()
        .insert(entry_id, result.credential.credential_id());

    core.touch_activity();
    Ok(RegisterPasskeyResult {
        entry_id: node_id(entry_id),
        credential_id: passkeys::encode(&result.response.credential_id),
        authenticator_data: passkeys::encode(&result.response.authenticator_data),
    })
}

/// Every field of the freshly added entry, as `prepare_entry_update` requires the
/// complete field set including all standard fields.
fn entry_fields(
    title: &str,
    user_name: &str,
    credential: &PasskeyCredential,
) -> Result<Vec<update_entry::JournalEntryField>> {
    let mut fields = vec![
        standard_field(StandardField::Title, Some(title.to_string())),
        standard_field(StandardField::UserName, Some(user_name.to_string())),
        // Passkey entries hold no password; `None` preserves the protected empty value.
        update_entry::JournalEntryField {
            field_id: Some(standard_field_id(StandardField::Password)),
            name: StandardField::Password.name().to_string(),
            value: None,
            is_protected: true,
        },
        standard_field(
            StandardField::Url,
            Some(format!("https://{}", credential.rp_id())),
        ),
        standard_field(StandardField::Notes, Some(String::new())),
    ];
    fields.extend(credential.to_field_values()?.into_iter().map(|field| {
        update_entry::JournalEntryField {
            field_id: None,
            name: field.name.to_string(),
            value: Some(field.value.to_string()),
            is_protected: field.protected,
        }
    }));
    Ok(fields)
}

fn standard_field_id(field: StandardField) -> String {
    EntryFieldId::Standard(field).to_string()
}

fn standard_field(field: StandardField, value: Option<String>) -> update_entry::JournalEntryField {
    update_entry::JournalEntryField {
        field_id: Some(standard_field_id(field)),
        name: field.name().to_string(),
        value,
        is_protected: false,
    }
}

pub(crate) async fn execute(
    core: &mut KeelessCore,
    args: RegisterPasskeyArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::RegisterPasskey(run(core, args).await?))
}
