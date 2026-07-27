use keeless_kdbx::{
    CtapRegistrationRequest, EntryFieldId, PasskeyAuthenticator, PasskeyCredential, StandardField,
};
use keeless_schema::{
    AddEntryArgs, DeleteEntryArgs, EntryFieldUpdate, OperationSuccess, RegisterPasskeyArgs,
    RegisterPasskeyResult,
};

use crate::features::passkeys;
use crate::model::node_id;
use crate::{CoreError, KeelessCore, PasswordInputMode, Result};

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

    let unlocked = passkeys::unlock(core, PasswordInputMode::Save).await?;
    let existing = passkeys::visible_credentials(core, &unlocked.key, Some(&args.rp_id))?;
    let existing_credentials = existing
        .iter()
        .map(|(_, credential)| credential)
        .collect::<Vec<_>>();
    let result = PasskeyAuthenticator::create_ctap(&CtapRegistrationRequest {
        client_data_hash: &client_data_hash,
        rp_id: &args.rp_id,
        user_handle: &user_handle,
        username: &args.user_name,
        algorithms: &args.algorithms,
        existing_credentials: &existing_credentials,
        exclude_credential_ids: &exclude_credential_ids
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>(),
        user_verification: passkeys::user_verification(args.user_verified),
    })?;
    drop(existing_credentials);
    drop(existing);

    let entry_id = super::mutations::add_entry::run(
        core,
        AddEntryArgs {
            parent_group_id: node_id(parent_group_id),
        },
    )
    .await?
    .id;

    let title = args
        .rp_name
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or(result.credential.rp_id());
    let fields = entry_fields(title, &args.user_name, &result.credential)?;
    if let Err(error) = super::mutations::update_entry::run(
        core,
        entry_id.clone(),
        fields,
        None,
        unlocked.password(),
    )
    .await
    {
        // The entry was already journaled, so leave the database consistent by
        // trashing the stub rather than leaving an empty entry in the root group.
        let _ = super::mutations::delete_entry::run(
            core,
            DeleteEntryArgs {
                entry_id: entry_id.clone(),
                permanent: false,
            },
        )
        .await;
        return Err(error);
    }

    core.touch_activity();
    Ok(RegisterPasskeyResult {
        entry_id,
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
) -> Result<Vec<EntryFieldUpdate>> {
    let mut fields = vec![
        standard_field(StandardField::Title, Some(title.to_string())),
        standard_field(StandardField::UserName, Some(user_name.to_string())),
        // Passkey entries hold no password; `None` preserves the protected empty value.
        EntryFieldUpdate {
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
    fields.extend(
        credential
            .to_field_values()?
            .into_iter()
            .map(|field| EntryFieldUpdate {
                field_id: None,
                name: field.name.to_string(),
                value: Some(field.value.to_string()),
                is_protected: field.protected,
            }),
    );
    Ok(fields)
}

fn standard_field_id(field: StandardField) -> String {
    EntryFieldId::Standard(field).to_string()
}

fn standard_field(field: StandardField, value: Option<String>) -> EntryFieldUpdate {
    EntryFieldUpdate {
        field_id: Some(standard_field_id(field)),
        name: field.name().to_string(),
        value,
        is_protected: false,
    }
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    args: RegisterPasskeyArgs,
) -> Result<OperationSuccess> {
    Ok(OperationSuccess::RegisterPasskey(run(core, args).await?))
}
