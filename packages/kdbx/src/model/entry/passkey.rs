//! WebAuthn passkey creation, authentication, and KeePassXC field storage.

mod key;
mod webauthn;

#[cfg(test)]
mod test;

use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::model::core::node::NodeId;
use crate::model::core::security::ProtectedString;
use crate::model::db::{CompositeKey, Database};
use crate::model::entry::Entry;

use key::CredentialKey;

pub use webauthn::KEELESS_AAGUID;

/// Byte length of the client data hash CTAP2 platforms supply.
const CLIENT_DATA_HASH_LENGTH: usize = 32;

pub const FIELD_USERNAME: &str = "KPEX_PASSKEY_USERNAME";
pub const FIELD_CREDENTIAL_ID: &str = "KPEX_PASSKEY_CREDENTIAL_ID";
pub const FIELD_PRIVATE_KEY_PEM: &str = "KPEX_PASSKEY_PRIVATE_KEY_PEM";
pub const FIELD_RELYING_PARTY: &str = "KPEX_PASSKEY_RELYING_PARTY";
pub const FIELD_USER_HANDLE: &str = "KPEX_PASSKEY_USER_HANDLE";
pub const FIELD_BACKUP_ELIGIBLE: &str = "KPEX_PASSKEY_FLAG_BE";
pub const FIELD_BACKUP_STATE: &str = "KPEX_PASSKEY_FLAG_BS";
pub const FIELD_GENERATED_USER_ID: &str = "KPEX_PASSKEY_GENERATED_USER_ID";
pub const FIELD_COMPATIBLE_USERNAME: &str = "KPXC_PASSKEY_USERNAME";

/// Algorithms supported by the software authenticator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasskeyAlgorithm {
    Es256,
    Rs256,
    Ed25519,
}

impl PasskeyAlgorithm {
    pub const fn cose_id(self) -> i32 {
        match self {
            Self::Es256 => -7,
            Self::Rs256 => -257,
            Self::Ed25519 => -8,
        }
    }

    fn from_cose_id(value: i32) -> Option<Self> {
        match value {
            -7 => Some(Self::Es256),
            -257 => Some(Self::Rs256),
            -8 => Some(Self::Ed25519),
            _ => None,
        }
    }
}

/// Whether the host completed its user-verification policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UserVerification {
    #[default]
    NotVerified,
    Verified,
}

/// Inputs needed to create a resident passkey.
pub struct RegistrationRequest<'a> {
    pub client_data_json: &'a [u8],
    /// Origin obtained independently from the trusted browser boundary.
    pub origin: &'a str,
    /// Raw challenge expected from the relying party (at least 16 bytes).
    pub challenge: &'a [u8],
    pub rp_id: &'a str,
    pub user_handle: &'a [u8],
    pub username: &'a str,
    /// COSE algorithm identifiers in relying-party preference order.
    pub algorithms: &'a [i32],
    /// Credentials available to this authenticator. Callers must pass all
    /// resident credentials so `exclude_credential_ids` can be enforced.
    pub existing_credentials: &'a [&'a PasskeyCredential],
    /// Raw credential IDs from the WebAuthn `excludeCredentials` option.
    pub exclude_credential_ids: &'a [&'a [u8]],
    /// Result of the host's verification ceremony, not the RP preference.
    pub user_verification: UserVerification,
}

/// Inputs needed to produce an authentication assertion.
pub struct AuthenticationRequest<'a> {
    pub client_data_json: &'a [u8],
    /// Origin obtained independently from the trusted browser boundary.
    pub origin: &'a str,
    /// Raw challenge expected from the relying party (at least 16 bytes).
    pub challenge: &'a [u8],
    pub rp_id: &'a str,
    /// Empty means any discoverable credential is allowed.
    pub allowed_credential_ids: &'a [&'a [u8]],
    /// Result of the host's verification ceremony, not the RP preference.
    pub user_verification: UserVerification,
}

/// Inputs needed to create a resident passkey from a CTAP2 `authenticatorMakeCredential`.
///
/// CTAP2 platforms hash the client data themselves and validate the origin against
/// the relying party, so neither is available here.
pub struct CtapRegistrationRequest<'a> {
    /// SHA-256 of the collected client data, exactly 32 bytes.
    pub client_data_hash: &'a [u8],
    pub rp_id: &'a str,
    pub user_handle: &'a [u8],
    pub username: &'a str,
    /// COSE algorithm identifiers in relying-party preference order.
    pub algorithms: &'a [i32],
    /// Credentials available to this authenticator. Callers must pass all
    /// resident credentials so `exclude_credential_ids` can be enforced.
    pub existing_credentials: &'a [&'a PasskeyCredential],
    /// Raw credential IDs from the CTAP2 `excludeList` parameter.
    pub exclude_credential_ids: &'a [&'a [u8]],
    /// Result of the host's verification ceremony, not the RP preference.
    pub user_verification: UserVerification,
}

/// Inputs needed to produce an assertion from a CTAP2 `authenticatorGetAssertion`.
pub struct CtapAuthenticationRequest<'a> {
    /// SHA-256 of the collected client data, exactly 32 bytes.
    pub client_data_hash: &'a [u8],
    pub rp_id: &'a str,
    /// Empty means any discoverable credential is allowed.
    pub allowed_credential_ids: &'a [&'a [u8]],
    /// Result of the host's verification ceremony, not the RP preference.
    pub user_verification: UserVerification,
}

/// Signed members of a CTAP2 `authenticatorMakeCredential` response.
///
/// Callers wrap `authenticator_data` in the attestation map themselves, because
/// CTAP2 responses key that map by integer while WebAuthn keys it by string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CtapRegistrationResponse {
    pub credential_id: Vec<u8>,
    pub authenticator_data: Vec<u8>,
    pub public_key_spki: Vec<u8>,
    pub public_key_algorithm: i32,
}

/// Signed members of a CTAP2 `authenticatorGetAssertion` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CtapAuthenticationResponse {
    pub credential_id: Vec<u8>,
    pub authenticator_data: Vec<u8>,
    pub signature: Vec<u8>,
    pub user_handle: Vec<u8>,
}

/// A newly-created credential and its CTAP2 registration response.
pub struct CtapRegistrationResult {
    pub credential: PasskeyCredential,
    pub response: CtapRegistrationResponse,
}

/// One KPEX field to persist for a credential.
pub struct PasskeyFieldValue {
    pub name: &'static str,
    pub value: Zeroizing<String>,
    pub protected: bool,
}

/// Binary members of an AuthenticatorAttestationResponse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationResponse {
    pub credential_id: Vec<u8>,
    pub client_data_json: Vec<u8>,
    pub attestation_object: Vec<u8>,
    pub authenticator_data: Vec<u8>,
    pub public_key_spki: Vec<u8>,
    pub public_key_algorithm: i32,
}

/// Binary members of an AuthenticatorAssertionResponse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticationResponse {
    pub credential_id: Vec<u8>,
    pub client_data_json: Vec<u8>,
    pub authenticator_data: Vec<u8>,
    pub signature: Vec<u8>,
    pub user_handle: Vec<u8>,
}

/// A newly-created credential and its registration response.
pub struct RegistrationResult {
    pub credential: PasskeyCredential,
    pub response: RegistrationResponse,
}

/// A passkey credential suitable for KPEX persistence and WebAuthn assertions.
pub struct PasskeyCredential {
    rp_id: String,
    username: String,
    credential_id: Vec<u8>,
    user_handle: Vec<u8>,
    backup_eligible: bool,
    backup_state: bool,
    key: CredentialKey,
}

impl PasskeyCredential {
    pub fn rp_id(&self) -> &str {
        &self.rp_id
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn credential_id(&self) -> &[u8] {
        &self.credential_id
    }

    pub fn user_handle(&self) -> &[u8] {
        &self.user_handle
    }

    pub fn algorithm(&self) -> PasskeyAlgorithm {
        self.key.algorithm()
    }

    pub fn backup_eligible(&self) -> bool {
        self.backup_eligible
    }

    pub fn backup_state(&self) -> bool {
        self.backup_state
    }

    /// Parse a complete KPEX passkey from an entry.
    pub fn from_entry(entry: &Entry) -> Result<Option<Self>, PasskeyError> {
        if !is_passkey_entry(entry) {
            return Ok(None);
        }

        let credential_id_name = if has_field(entry, FIELD_GENERATED_USER_ID) {
            FIELD_GENERATED_USER_ID
        } else {
            FIELD_CREDENTIAL_ID
        };
        let username_name = if has_field(entry, FIELD_COMPATIBLE_USERNAME) {
            FIELD_COMPATIBLE_USERNAME
        } else {
            FIELD_USERNAME
        };

        let credential_id = decode_identifier(
            required_field(entry, credential_id_name)?,
            credential_id_name,
        )?;
        let user_handle =
            decode_identifier(required_field(entry, FIELD_USER_HANDLE)?, FIELD_USER_HANDLE)?;
        validate_user_handle(&user_handle)?;
        let rp_id = webauthn::normalize_stored_rp_id(required_field(entry, FIELD_RELYING_PARTY)?)?;
        let username = required_field(entry, username_name)?.to_string();
        let private_key_pem = required_field(entry, FIELD_PRIVATE_KEY_PEM)?;
        let key = CredentialKey::from_pkcs8_pem(private_key_pem)?;
        let backup_eligible = parse_flag(entry, FIELD_BACKUP_ELIGIBLE, true)?;
        let backup_state = parse_flag(entry, FIELD_BACKUP_STATE, true)?;
        if backup_state && !backup_eligible {
            return Err(PasskeyError::InvalidBackupFlags);
        }

        Ok(Some(Self {
            rp_id,
            username,
            credential_id,
            user_handle,
            backup_eligible,
            backup_state,
            key,
        }))
    }

    /// Parse a passkey from a loaded database without retaining field plaintext.
    pub fn from_database_entry(
        database: &Database,
        composite_key: &CompositeKey,
        entry_id: &NodeId,
    ) -> Result<Option<Self>, PasskeyError> {
        let entry = database
            .entries
            .get(entry_id)
            .ok_or(PasskeyError::ProtectedFieldAccess)?;
        let mut unlock = database.memory_unlock(composite_key);
        let entry = entry
            .semantic_clone(&mut unlock)
            .map_err(|_| PasskeyError::ProtectedFieldAccess)?;
        Self::from_entry(&entry)
    }

    /// Store this credential unless the entry already contains passkey fields.
    pub fn store_in_entry(&self, entry: &mut Entry) -> Result<(), PasskeyError> {
        if is_passkey_entry(entry) {
            return Err(PasskeyError::PasskeyAlreadyExists);
        }
        self.write_fields(entry)
    }

    /// Store this credential and immediately seal its protected fields.
    pub fn store_in_database(
        &self,
        database: &mut Database,
        composite_key: &CompositeKey,
        entry_id: &NodeId,
    ) -> Result<(), PasskeyError> {
        let entry = database
            .entries
            .get_mut(entry_id)
            .ok_or(PasskeyError::ProtectedFieldAccess)?;
        self.store_in_entry(entry)?;
        database
            .protect_entry_strings(composite_key)
            .map_err(|_| PasskeyError::ProtectedFieldAccess)
    }

    /// Explicitly replace all passkey fields in an entry.
    pub fn replace_in_entry(&self, entry: &mut Entry) -> Result<(), PasskeyError> {
        Self::remove_from_entry(entry);
        self.write_fields(entry)
    }

    /// Remove canonical, future KPEX, and known compatibility passkey fields.
    pub fn remove_from_entry(entry: &mut Entry) {
        entry.retain_custom_fields(|field| {
            !field.name.starts_with("KPEX_PASSKEY") && field.name != FIELD_COMPATIBLE_USERNAME
        });
    }

    /// Create a WebAuthn assertion for this credential.
    pub fn authenticate(
        &self,
        request: &AuthenticationRequest<'_>,
    ) -> Result<AuthenticationResponse, PasskeyError> {
        let rp_id = webauthn::validate_client_data(
            request.client_data_json,
            request.origin,
            request.challenge,
            request.rp_id,
            webauthn::Ceremony::Get,
        )?;
        if rp_id != self.rp_id {
            return Err(PasskeyError::RpIdMismatch);
        }

        let response = self.sign_assertion(
            &Sha256::digest(request.client_data_json),
            request.allowed_credential_ids,
            request.user_verification,
        )?;

        Ok(AuthenticationResponse {
            credential_id: response.credential_id,
            client_data_json: request.client_data_json.to_vec(),
            authenticator_data: response.authenticator_data,
            signature: response.signature,
            user_handle: response.user_handle,
        })
    }

    /// Create an assertion for a CTAP2 `authenticatorGetAssertion` request.
    pub fn authenticate_ctap(
        &self,
        request: &CtapAuthenticationRequest<'_>,
    ) -> Result<CtapAuthenticationResponse, PasskeyError> {
        validate_client_data_hash(request.client_data_hash)?;
        if webauthn::normalize_stored_rp_id(request.rp_id)? != self.rp_id {
            return Err(PasskeyError::RpIdMismatch);
        }
        self.sign_assertion(
            request.client_data_hash,
            request.allowed_credential_ids,
            request.user_verification,
        )
    }

    /// Fields this credential occupies in an entry, in persistence order.
    pub fn to_field_values(&self) -> Result<Vec<PasskeyFieldValue>, PasskeyError> {
        Ok(vec![
            plain_value(FIELD_USERNAME, self.username.clone()),
            protected_value(
                FIELD_CREDENTIAL_ID,
                URL_SAFE_NO_PAD.encode(&self.credential_id),
            ),
            PasskeyFieldValue {
                name: FIELD_PRIVATE_KEY_PEM,
                value: self.key.to_pkcs8_pem()?,
                protected: true,
            },
            plain_value(FIELD_RELYING_PARTY, self.rp_id.clone()),
            protected_value(FIELD_USER_HANDLE, URL_SAFE_NO_PAD.encode(&self.user_handle)),
            plain_value(FIELD_BACKUP_ELIGIBLE, flag_value(self.backup_eligible)),
            plain_value(FIELD_BACKUP_STATE, flag_value(self.backup_state)),
        ])
    }

    fn sign_assertion(
        &self,
        client_data_hash: &[u8],
        allowed_credential_ids: &[&[u8]],
        user_verification: UserVerification,
    ) -> Result<CtapAuthenticationResponse, PasskeyError> {
        if !allowed_credential_ids.is_empty()
            && !allowed_credential_ids
                .iter()
                .any(|candidate| *candidate == self.credential_id)
        {
            return Err(PasskeyError::CredentialNotAllowed);
        }

        let authenticator_data = webauthn::assertion_authenticator_data(
            &self.rp_id,
            user_verification,
            self.backup_eligible,
            self.backup_state,
        );
        let message = webauthn::assertion_signature_message(&authenticator_data, client_data_hash);
        let signature = self.key.sign(&message)?;

        Ok(CtapAuthenticationResponse {
            credential_id: self.credential_id.clone(),
            authenticator_data,
            signature,
            user_handle: self.user_handle.clone(),
        })
    }

    fn write_fields(&self, entry: &mut Entry) -> Result<(), PasskeyError> {
        for field in self.to_field_values()? {
            let value = if field.protected {
                ProtectedString::new_protected(&field.value)
            } else {
                ProtectedString::new_plain(&field.value)
            };
            entry.add_custom_field(field.name, value);
        }
        Ok(())
    }
}

/// Stateless passkey registration operations, analogous to the OTP calculator.
pub struct PasskeyAuthenticator;

impl PasskeyAuthenticator {
    pub fn create(request: &RegistrationRequest<'_>) -> Result<RegistrationResult, PasskeyError> {
        let rp_id = webauthn::validate_client_data(
            request.client_data_json,
            request.origin,
            request.challenge,
            request.rp_id,
            webauthn::Ceremony::Create,
        )?;
        let (credential, response) = Self::create_credential(CredentialInputs {
            rp_id,
            user_handle: request.user_handle,
            username: request.username,
            algorithms: request.algorithms,
            existing_credentials: request.existing_credentials,
            exclude_credential_ids: request.exclude_credential_ids,
            user_verification: request.user_verification,
        })?;
        let attestation_object = webauthn::none_attestation(&response.authenticator_data)?;

        Ok(RegistrationResult {
            credential,
            response: RegistrationResponse {
                credential_id: response.credential_id,
                client_data_json: request.client_data_json.to_vec(),
                attestation_object,
                authenticator_data: response.authenticator_data,
                public_key_spki: response.public_key_spki,
                public_key_algorithm: response.public_key_algorithm,
            },
        })
    }

    /// Create a resident credential for a CTAP2 `authenticatorMakeCredential` request.
    pub fn create_ctap(
        request: &CtapRegistrationRequest<'_>,
    ) -> Result<CtapRegistrationResult, PasskeyError> {
        validate_client_data_hash(request.client_data_hash)?;
        let (credential, response) = Self::create_credential(CredentialInputs {
            rp_id: webauthn::normalize_stored_rp_id(request.rp_id)?,
            user_handle: request.user_handle,
            username: request.username,
            algorithms: request.algorithms,
            existing_credentials: request.existing_credentials,
            exclude_credential_ids: request.exclude_credential_ids,
            user_verification: request.user_verification,
        })?;

        Ok(CtapRegistrationResult {
            credential,
            response,
        })
    }

    fn create_credential(
        inputs: CredentialInputs<'_>,
    ) -> Result<(PasskeyCredential, CtapRegistrationResponse), PasskeyError> {
        validate_user_handle(inputs.user_handle)?;
        if inputs.username.is_empty() {
            return Err(PasskeyError::EmptyUsername);
        }
        let algorithm = inputs
            .algorithms
            .iter()
            .find_map(|value| PasskeyAlgorithm::from_cose_id(*value))
            .ok_or(PasskeyError::UnsupportedAlgorithm)?;
        if inputs.existing_credentials.iter().any(|credential| {
            credential.rp_id == inputs.rp_id
                && inputs
                    .exclude_credential_ids
                    .iter()
                    .any(|excluded| *excluded == credential.credential_id)
        }) {
            return Err(PasskeyError::CredentialExcluded);
        }

        let key = CredentialKey::generate(algorithm)?;
        let mut credential_id = vec![0u8; 32];
        getrandom::getrandom(&mut credential_id).map_err(|_| PasskeyError::RandomGeneration)?;
        let public_key_cose = key.to_cose_key()?;
        let authenticator_data = webauthn::registration_authenticator_data(
            &inputs.rp_id,
            inputs.user_verification,
            true,
            true,
            &credential_id,
            &public_key_cose,
        )?;
        let public_key_spki = key.public_key_spki()?;

        let response = CtapRegistrationResponse {
            credential_id: credential_id.clone(),
            authenticator_data,
            public_key_spki,
            public_key_algorithm: algorithm.cose_id(),
        };
        let credential = PasskeyCredential {
            rp_id: inputs.rp_id,
            username: inputs.username.to_string(),
            credential_id,
            user_handle: inputs.user_handle.to_vec(),
            backup_eligible: true,
            backup_state: true,
            key,
        };

        Ok((credential, response))
    }
}

/// Registration inputs left after each protocol validated its own client data.
struct CredentialInputs<'a> {
    rp_id: String,
    user_handle: &'a [u8],
    username: &'a str,
    algorithms: &'a [i32],
    existing_credentials: &'a [&'a PasskeyCredential],
    exclude_credential_ids: &'a [&'a [u8]],
    user_verification: UserVerification,
}

/// Collect every passkey credential stored in a loaded database.
///
/// Passing `rp_id` limits the result to one relying party. Entries whose passkey
/// fields are malformed are skipped so a single broken entry cannot hide the rest;
/// a failure to read protected memory aborts the scan because it affects every entry.
pub fn find_credentials(
    database: &Database,
    composite_key: &CompositeKey,
    rp_id: Option<&str>,
) -> Result<Vec<(NodeId, PasskeyCredential)>, PasskeyError> {
    let rp_filter = rp_id.map(webauthn::normalize_stored_rp_id).transpose()?;
    let mut unlock = database.memory_unlock(composite_key);
    let mut credentials = Vec::new();

    for (id, entry) in &database.entries {
        if !is_passkey_entry(entry) {
            continue;
        }
        let entry = entry
            .semantic_clone(&mut unlock)
            .map_err(|_| PasskeyError::ProtectedFieldAccess)?;
        let Ok(Some(credential)) = PasskeyCredential::from_entry(&entry) else {
            continue;
        };
        if rp_filter
            .as_deref()
            .is_some_and(|filter| filter != credential.rp_id)
        {
            continue;
        }
        credentials.push((*id, credential));
    }

    credentials.sort_by(|(_, left), (_, right)| left.credential_id.cmp(&right.credential_id));
    Ok(credentials)
}

/// Detect any canonical or future KPEX passkey field.
pub fn is_passkey_entry(entry: &Entry) -> bool {
    entry
        .custom_fields()
        .any(|(_, field)| field.name.starts_with("KPEX_PASSKEY"))
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PasskeyError {
    #[error("invalid client data JSON")]
    InvalidClientData,
    #[error("client data hash must contain exactly 32 bytes")]
    InvalidClientDataHash,
    #[error("WebAuthn challenges must contain at least 16 bytes")]
    ChallengeTooShort,
    #[error("client data challenge does not match the trusted challenge")]
    ChallengeMismatch,
    #[error("unexpected WebAuthn ceremony type")]
    InvalidCeremonyType,
    #[error("invalid or insecure WebAuthn origin")]
    InvalidOrigin,
    #[error("client data origin does not match the trusted origin")]
    OriginMismatch,
    #[error("cross-origin WebAuthn ceremonies are not supported")]
    CrossOriginNotSupported,
    #[error("invalid relying-party identifier")]
    InvalidRpId,
    #[error("relying-party identifier does not match the origin")]
    RpIdOriginMismatch,
    #[error("stored credential belongs to a different relying party")]
    RpIdMismatch,
    #[error("unsupported passkey algorithm")]
    UnsupportedAlgorithm,
    #[error("user handle must contain between 1 and 64 bytes")]
    InvalidUserHandleLength,
    #[error("username must not be empty")]
    EmptyUsername,
    #[error("credential is not included in allowCredentials")]
    CredentialNotAllowed,
    #[error("an excluded credential already exists")]
    CredentialExcluded,
    #[error("passkey already exists in the entry")]
    PasskeyAlreadyExists,
    #[error("missing passkey field: {0}")]
    MissingField(&'static str),
    #[error("duplicate passkey field: {0}")]
    DuplicateField(&'static str),
    #[error("invalid passkey identifier: {0}")]
    InvalidIdentifier(&'static str),
    #[error("invalid passkey flag: {0}")]
    InvalidFlag(&'static str),
    #[error("backup state requires backup eligibility")]
    InvalidBackupFlags,
    #[error("invalid passkey private key")]
    InvalidPrivateKey,
    #[error("passkey key generation failed")]
    KeyGeneration,
    #[error("passkey signing failed")]
    Signing,
    #[error("random generation failed")]
    RandomGeneration,
    #[error("CBOR encoding failed")]
    CborEncoding,
    #[error("public key encoding failed")]
    PublicKeyEncoding,
    #[error("protected passkey fields could not be unlocked")]
    ProtectedFieldAccess,
}

fn validate_user_handle(user_handle: &[u8]) -> Result<(), PasskeyError> {
    if !(1..=64).contains(&user_handle.len()) {
        return Err(PasskeyError::InvalidUserHandleLength);
    }
    Ok(())
}

fn validate_client_data_hash(client_data_hash: &[u8]) -> Result<(), PasskeyError> {
    if client_data_hash.len() != CLIENT_DATA_HASH_LENGTH {
        return Err(PasskeyError::InvalidClientDataHash);
    }
    Ok(())
}

fn has_field(entry: &Entry, name: &str) -> bool {
    entry.custom_fields().any(|(_, field)| field.name == name)
}

fn required_field<'a>(entry: &'a Entry, name: &'static str) -> Result<&'a str, PasskeyError> {
    let mut fields = entry
        .custom_fields()
        .filter(|(_, field)| field.name == name)
        .map(|(_, field)| field);
    let value = fields
        .next()
        .ok_or(PasskeyError::MissingField(name))?
        .value
        .as_str();
    if fields.next().is_some() {
        return Err(PasskeyError::DuplicateField(name));
    }
    if value.is_empty() {
        return Err(PasskeyError::MissingField(name));
    }
    Ok(value)
}

fn decode_identifier(value: &str, field: &'static str) -> Result<Vec<u8>, PasskeyError> {
    if value.is_empty() || value.contains(['+', '/']) {
        return Err(PasskeyError::InvalidIdentifier(field));
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .or_else(|_| URL_SAFE.decode(value))
        .map_err(|_| PasskeyError::InvalidIdentifier(field))?;
    if decoded.is_empty() {
        return Err(PasskeyError::InvalidIdentifier(field));
    }
    Ok(decoded)
}

fn parse_flag(entry: &Entry, name: &'static str, default: bool) -> Result<bool, PasskeyError> {
    if !has_field(entry, name) {
        return Ok(default);
    }
    match required_field(entry, name)?.to_ascii_lowercase().as_str() {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err(PasskeyError::InvalidFlag(name)),
    }
}

fn plain_value(name: &'static str, value: String) -> PasskeyFieldValue {
    PasskeyFieldValue {
        name,
        value: Zeroizing::new(value),
        protected: false,
    }
}

fn protected_value(name: &'static str, value: String) -> PasskeyFieldValue {
    PasskeyFieldValue {
        name,
        value: Zeroizing::new(value),
        protected: true,
    }
}

fn flag_value(value: bool) -> String {
    if value { "1" } else { "0" }.to_string()
}
