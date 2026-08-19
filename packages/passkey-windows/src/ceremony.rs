//! Translation from validated Windows SDK request fields into Core operations.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use keeless_passkey_ctap::response::{
    MAX_CREDENTIAL_COUNT_IN_LIST, MAX_CREDENTIAL_ID_LENGTH, SUPPORTED_ALGORITHMS,
};
use keeless_schema::{AssertPasskeyArgs, Operation, RegisterPasskeyArgs};

const CLIENT_DATA_HASH_LENGTH: usize = 32;

/// Fields copied from a decoded `authenticatorMakeCredential` request.
#[derive(Debug, Eq, PartialEq)]
pub struct MakeCredentialRequest {
    pub rp_id: String,
    pub rp_name: Option<String>,
    pub user_name: String,
    pub user_handle: Vec<u8>,
    pub client_data_hash: Vec<u8>,
    pub algorithms: Vec<i32>,
    pub exclude_credential_ids: Vec<Vec<u8>>,
}

/// Fields copied from a decoded `authenticatorGetAssertion` request.
#[derive(Debug, Eq, PartialEq)]
pub struct GetAssertionRequest {
    pub rp_id: String,
    pub client_data_hash: Vec<u8>,
    pub allow_credential_ids: Vec<Vec<u8>>,
    /// A request without user presence is conditional/silent discovery.
    pub user_presence: bool,
}

/// A Windows silent discovery request must end without contacting Core.
pub enum AssertionOperation {
    SilentDiscovery,
    Interactive(Box<Operation>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RequestError {
    #[error("request has an invalid required field")]
    InvalidParameter,
    #[error("request has no supported credential algorithm")]
    UnsupportedAlgorithm,
}

/// Convert a signature-verified make-credential request for the desktop host.
pub fn make_credential_operation(
    request: MakeCredentialRequest,
) -> Result<Operation, RequestError> {
    validate_rp_id(&request.rp_id)?;
    validate_client_data_hash(&request.client_data_hash)?;
    if request.user_name.is_empty() || request.user_handle.is_empty() {
        return Err(RequestError::InvalidParameter);
    }
    validate_credential_ids(&request.exclude_credential_ids)?;
    if !request
        .algorithms
        .iter()
        .any(|algorithm| SUPPORTED_ALGORITHMS.contains(algorithm))
    {
        return Err(RequestError::UnsupportedAlgorithm);
    }

    Ok(Operation::RegisterPasskey(RegisterPasskeyArgs {
        rp_id: request.rp_id,
        rp_name: request.rp_name,
        user_name: request.user_name,
        user_handle: encode(&request.user_handle),
        client_data_hash: encode(&request.client_data_hash),
        algorithms: request.algorithms,
        exclude_credential_ids: request
            .exclude_credential_ids
            .iter()
            .map(|credential_id| encode(credential_id))
            .collect(),
        password_session: None,
    }))
}

/// Convert a signature-verified assertion request for the desktop host.
pub fn get_assertion_operation(
    request: GetAssertionRequest,
) -> Result<AssertionOperation, RequestError> {
    validate_rp_id(&request.rp_id)?;
    validate_client_data_hash(&request.client_data_hash)?;
    validate_credential_ids(&request.allow_credential_ids)?;
    if !request.user_presence {
        return Ok(AssertionOperation::SilentDiscovery);
    }

    Ok(AssertionOperation::Interactive(Box::new(
        Operation::AssertPasskey(AssertPasskeyArgs {
            rp_id: request.rp_id,
            client_data_hash: encode(&request.client_data_hash),
            allow_credential_ids: request
                .allow_credential_ids
                .iter()
                .map(|credential_id| encode(credential_id))
                .collect(),
            // Windows Hello has already completed at this point. Core still owns
            // the existing database unlock and Keeless consent flows.
            user_present: true,
            password_session: None,
        }),
    )))
}

fn validate_rp_id(rp_id: &str) -> Result<(), RequestError> {
    if rp_id.is_empty() {
        return Err(RequestError::InvalidParameter);
    }
    Ok(())
}

fn validate_client_data_hash(client_data_hash: &[u8]) -> Result<(), RequestError> {
    if client_data_hash.len() != CLIENT_DATA_HASH_LENGTH {
        return Err(RequestError::InvalidParameter);
    }
    Ok(())
}

fn validate_credential_ids(credential_ids: &[Vec<u8>]) -> Result<(), RequestError> {
    if credential_ids.len() > MAX_CREDENTIAL_COUNT_IN_LIST as usize
        || credential_ids.iter().any(|credential_id| {
            credential_id.is_empty() || credential_id.len() > MAX_CREDENTIAL_ID_LENGTH as usize
        })
    {
        return Err(RequestError::InvalidParameter);
    }
    Ok(())
}

fn encode(value: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_make_credential_fields_without_reencoding_ctap() {
        let operation = make_credential_operation(MakeCredentialRequest {
            rp_id: "example.com".into(),
            rp_name: Some("Example".into()),
            user_name: "alice".into(),
            user_handle: vec![1, 2, 3],
            client_data_hash: vec![4; 32],
            algorithms: vec![-36, -7],
            exclude_credential_ids: vec![vec![5, 6]],
        })
        .unwrap();
        let Operation::RegisterPasskey(args) = operation else {
            panic!("expected registration operation");
        };
        assert_eq!(args.rp_id, "example.com");
        assert_eq!(args.rp_name.as_deref(), Some("Example"));
        assert_eq!(args.user_handle, "AQID");
        assert_eq!(
            args.client_data_hash,
            "BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ"
        );
        assert_eq!(args.algorithms, vec![-36, -7]);
        assert_eq!(args.exclude_credential_ids, vec!["BQY"]);
    }

    #[test]
    fn rejects_unsupported_or_malformed_registration_inputs() {
        let request = || MakeCredentialRequest {
            rp_id: "example.com".into(),
            rp_name: None,
            user_name: "alice".into(),
            user_handle: vec![1],
            client_data_hash: vec![2; 32],
            algorithms: vec![-7],
            exclude_credential_ids: Vec::new(),
        };
        let mut unsupported = request();
        unsupported.algorithms = vec![-36];
        assert!(matches!(
            make_credential_operation(unsupported),
            Err(RequestError::UnsupportedAlgorithm)
        ));

        let mut malformed = request();
        malformed.client_data_hash.pop();
        assert!(matches!(
            make_credential_operation(malformed),
            Err(RequestError::InvalidParameter)
        ));

        let mut oversized_credential_id = request();
        oversized_credential_id.exclude_credential_ids = vec![vec![0; 129]];
        assert!(matches!(
            make_credential_operation(oversized_credential_id),
            Err(RequestError::InvalidParameter)
        ));
    }

    #[test]
    fn refuses_silent_discovery_without_contacting_core() {
        let request = GetAssertionRequest {
            rp_id: "example.com".into(),
            client_data_hash: vec![3; 32],
            allow_credential_ids: Vec::new(),
            user_presence: false,
        };
        assert!(matches!(
            get_assertion_operation(request).unwrap(),
            AssertionOperation::SilentDiscovery
        ));
    }

    #[test]
    fn maps_interactive_assertion_to_a_user_present_core_operation() {
        let operation = get_assertion_operation(GetAssertionRequest {
            rp_id: "example.com".into(),
            client_data_hash: vec![7; 32],
            allow_credential_ids: vec![vec![8, 9]],
            user_presence: true,
        })
        .unwrap();
        let AssertionOperation::Interactive(operation) = operation else {
            panic!("expected interactive assertion operation");
        };
        let Operation::AssertPasskey(args) = *operation else {
            panic!("expected assertion operation");
        };
        assert!(args.user_present);
        assert_eq!(args.allow_credential_ids, vec!["CAk"]);
    }
}
