use super::*;
use crate::model::db::database::DatabaseVersion;
use crate::model::group::Group;
use crate::{KdbxXmlReader, KdbxXmlWriter, Salsa20InnerStream};
use p256::ecdsa::signature::Verifier;
use p256::pkcs8::DecodePublicKey;

const CREATE_CHALLENGE: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const GET_CHALLENGE: &[u8] = &[
    16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
];
const CREATE_CLIENT_DATA: &[u8] = br#"{"type":"webauthn.create","challenge":"AAECAwQFBgcICQoLDA0ODw","origin":"https://login.example.com","crossOrigin":false}"#;
const GET_CLIENT_DATA: &[u8] = br#"{"type":"webauthn.get","challenge":"EBESExQVFhcYGRobHB0eHw","origin":"https://login.example.com","crossOrigin":false}"#;
const CTAP_CLIENT_DATA_HASH: &[u8] = &[7u8; 32];

const ALL_ALGORITHMS: [PasskeyAlgorithm; 3] = [
    PasskeyAlgorithm::Es256,
    PasskeyAlgorithm::Rs256,
    PasskeyAlgorithm::Ed25519,
];

fn registration_request(algorithms: &[i32]) -> RegistrationRequest<'_> {
    RegistrationRequest {
        client_data_json: CREATE_CLIENT_DATA,
        origin: "https://login.example.com",
        challenge: CREATE_CHALLENGE,
        rp_id: "example.com",
        user_handle: b"user-handle",
        username: "alice",
        algorithms,
        existing_credentials: &[],
        exclude_credential_ids: &[],
        user_verification: UserVerification::Verified,
    }
}

fn ctap_registration_request(algorithms: &[i32]) -> CtapRegistrationRequest<'_> {
    CtapRegistrationRequest {
        client_data_hash: CTAP_CLIENT_DATA_HASH,
        rp_id: "example.com",
        user_handle: b"user-handle",
        username: "alice",
        algorithms,
        existing_credentials: &[],
        exclude_credential_ids: &[],
        user_verification: UserVerification::Verified,
    }
}

fn signed_message(authenticator_data: &[u8], client_data_hash: &[u8]) -> Vec<u8> {
    let mut message = authenticator_data.to_vec();
    message.extend_from_slice(client_data_hash);
    message
}

/// Verify an assertion against the SPKI public key from its registration.
fn verify_signature(
    algorithm: PasskeyAlgorithm,
    public_key_spki: &[u8],
    message: &[u8],
    signature: &[u8],
) {
    match algorithm {
        PasskeyAlgorithm::Es256 => {
            let public =
                p256::PublicKey::from_public_key_der(public_key_spki).expect("valid P-256 SPKI");
            let verifier = p256::ecdsa::VerifyingKey::from(public);
            let signature =
                p256::ecdsa::DerSignature::from_bytes(signature).expect("valid DER signature");
            verifier.verify(message, &signature).unwrap();
        }
        PasskeyAlgorithm::Rs256 => {
            let public =
                rsa::RsaPublicKey::from_public_key_der(public_key_spki).expect("valid RSA SPKI");
            let verifier = rsa::pkcs1v15::VerifyingKey::<Sha256>::new(public);
            let signature =
                rsa::pkcs1v15::Signature::try_from(signature).expect("valid RSA signature");
            verifier.verify(message, &signature).unwrap();
        }
        PasskeyAlgorithm::Ed25519 => {
            let verifier = ed25519_dalek::VerifyingKey::from_public_key_der(public_key_spki)
                .expect("valid Ed25519 SPKI");
            let signature =
                ed25519_dalek::Signature::from_slice(signature).expect("valid Ed25519 signature");
            verifier.verify(message, &signature).unwrap();
        }
    }
}

/// Assert the attested credential data embedded in registration authenticator data.
fn assert_attested_credential_data(
    algorithm: PasskeyAlgorithm,
    authenticator_data: &[u8],
    credential_id: &[u8],
) {
    assert_eq!(authenticator_data[32], 0x5d);
    assert_eq!(&authenticator_data[37..53], &KEELESS_AAGUID);
    assert_eq!(
        u16::from_be_bytes([authenticator_data[53], authenticator_data[54]]) as usize,
        credential_id.len()
    );

    let cose_offset = 55 + credential_id.len();
    let mut cose = minicbor::Decoder::new(&authenticator_data[cose_offset..]);
    assert!(cose.map().unwrap().is_some());
    assert_eq!(cose.i32().unwrap(), 1);
    let expected_key_type = match algorithm {
        PasskeyAlgorithm::Es256 => 2,
        PasskeyAlgorithm::Rs256 => 3,
        PasskeyAlgorithm::Ed25519 => 1,
    };
    assert_eq!(cose.i32().unwrap(), expected_key_type);
    assert_eq!(cose.i32().unwrap(), 3);
    assert_eq!(cose.i32().unwrap(), algorithm.cose_id());
}

fn database_with_entries(entries: Vec<Entry>) -> (Database, CompositeKey) {
    let root_id = NodeId::new_uuid();
    let mut root = Group::new(root_id);
    root.title = "Root".to_string();
    let mut database = Database::new(DatabaseVersion::KDBX4);
    for entry in entries {
        root.add_child_entry(entry.id);
        database.entries.insert(entry.id, entry);
    }
    database.root_group_id = Some(root_id);
    database.groups.insert(root_id, root);

    let composite_key = CompositeKey::new().with_password(b"test-password").unwrap();
    database
        .protect_entry_strings(&composite_key)
        .expect("protecting entry strings should succeed");
    (database, composite_key)
}

fn entry_with_credential(credential: &PasskeyCredential) -> Entry {
    let mut entry = Entry::new(NodeId::new_uuid());
    credential.store_in_entry(&mut entry).unwrap();
    entry
}

#[test]
fn creates_none_attestation_and_authenticates_with_all_algorithms() {
    for algorithm in ALL_ALGORITHMS {
        let result =
            PasskeyAuthenticator::create(&registration_request(&[12345, algorithm.cose_id()]))
                .expect("registration should succeed");

        assert_eq!(result.credential.algorithm(), algorithm);
        assert_eq!(result.response.public_key_algorithm, algorithm.cose_id());
        assert_eq!(result.response.client_data_json, CREATE_CLIENT_DATA);
        assert_attested_credential_data(
            algorithm,
            &result.response.authenticator_data,
            &result.response.credential_id,
        );

        let mut attestation = minicbor::Decoder::new(&result.response.attestation_object);
        assert_eq!(attestation.map().unwrap(), Some(3));
        assert_eq!(attestation.str().unwrap(), "fmt");
        assert_eq!(attestation.str().unwrap(), "none");
        assert_eq!(attestation.str().unwrap(), "attStmt");
        assert_eq!(attestation.map().unwrap(), Some(0));
        assert_eq!(attestation.str().unwrap(), "authData");
        assert_eq!(
            attestation.bytes().unwrap(),
            result.response.authenticator_data
        );

        let allowed = [result.credential.credential_id()];
        let assertion = result
            .credential
            .authenticate(&AuthenticationRequest {
                client_data_json: GET_CLIENT_DATA,
                origin: "https://login.example.com",
                challenge: GET_CHALLENGE,
                rp_id: "example.com",
                allowed_credential_ids: &allowed,
                user_verification: UserVerification::Verified,
            })
            .expect("authentication should succeed");
        assert_eq!(assertion.client_data_json, GET_CLIENT_DATA);
        assert_eq!(assertion.authenticator_data.len(), 37);
        assert_eq!(assertion.authenticator_data[32], 0x1d);
        assert_eq!(assertion.user_handle, b"user-handle");

        let message = signed_message(
            &assertion.authenticator_data,
            &Sha256::digest(GET_CLIENT_DATA),
        );
        verify_signature(
            algorithm,
            &result.response.public_key_spki,
            &message,
            &assertion.signature,
        );
    }
}

#[test]
fn ctap_registration_and_assertion_roundtrip_for_all_algorithms() {
    for algorithm in ALL_ALGORITHMS {
        let result = PasskeyAuthenticator::create_ctap(&ctap_registration_request(&[
            12345,
            algorithm.cose_id(),
        ]))
        .expect("CTAP registration should succeed");

        assert_eq!(result.credential.algorithm(), algorithm);
        assert_eq!(result.response.public_key_algorithm, algorithm.cose_id());
        assert_eq!(result.credential.rp_id(), "example.com");
        assert_attested_credential_data(
            algorithm,
            &result.response.authenticator_data,
            &result.response.credential_id,
        );

        let allowed = [result.credential.credential_id()];
        let assertion = result
            .credential
            .authenticate_ctap(&CtapAuthenticationRequest {
                client_data_hash: CTAP_CLIENT_DATA_HASH,
                rp_id: "EXAMPLE.com",
                allowed_credential_ids: &allowed,
                user_verification: UserVerification::Verified,
            })
            .expect("CTAP authentication should succeed");

        assert_eq!(assertion.credential_id, result.response.credential_id);
        assert_eq!(assertion.authenticator_data.len(), 37);
        assert_eq!(assertion.authenticator_data[32], 0x1d);
        assert_eq!(assertion.user_handle, b"user-handle");

        let message = signed_message(&assertion.authenticator_data, CTAP_CLIENT_DATA_HASH);
        verify_signature(
            algorithm,
            &result.response.public_key_spki,
            &message,
            &assertion.signature,
        );
    }
}

#[test]
fn ctap_requests_reject_malformed_hashes_and_relying_parties() {
    for length in [0, 31, 33] {
        let hash = vec![0u8; length];
        let error = PasskeyAuthenticator::create_ctap(&CtapRegistrationRequest {
            client_data_hash: &hash,
            ..ctap_registration_request(&[-7])
        })
        .err()
        .expect("client data hashes must be exactly 32 bytes");
        assert_eq!(error, PasskeyError::InvalidClientDataHash);
    }

    let result = PasskeyAuthenticator::create_ctap(&ctap_registration_request(&[-7])).unwrap();

    let short_hash = [0u8; 16];
    assert_eq!(
        result
            .credential
            .authenticate_ctap(&CtapAuthenticationRequest {
                client_data_hash: &short_hash,
                rp_id: "example.com",
                allowed_credential_ids: &[],
                user_verification: UserVerification::Verified,
            })
            .unwrap_err(),
        PasskeyError::InvalidClientDataHash
    );

    assert_eq!(
        result
            .credential
            .authenticate_ctap(&CtapAuthenticationRequest {
                client_data_hash: CTAP_CLIENT_DATA_HASH,
                rp_id: "example.net",
                allowed_credential_ids: &[],
                user_verification: UserVerification::Verified,
            })
            .unwrap_err(),
        PasskeyError::RpIdMismatch
    );

    let other_id = [b"other".as_slice()];
    assert_eq!(
        result
            .credential
            .authenticate_ctap(&CtapAuthenticationRequest {
                client_data_hash: CTAP_CLIENT_DATA_HASH,
                rp_id: "example.com",
                allowed_credential_ids: &other_id,
                user_verification: UserVerification::Verified,
            })
            .unwrap_err(),
        PasskeyError::CredentialNotAllowed
    );

    let invalid_rp_id = PasskeyAuthenticator::create_ctap(&CtapRegistrationRequest {
        rp_id: "example.com/path",
        ..ctap_registration_request(&[-7])
    })
    .err()
    .expect("invalid RP IDs should be rejected");
    assert_eq!(invalid_rp_id, PasskeyError::InvalidRpId);

    let unsupported = PasskeyAuthenticator::create_ctap(&CtapRegistrationRequest {
        algorithms: &[-36],
        ..ctap_registration_request(&[-7])
    })
    .err()
    .expect("unsupported algorithms should be rejected");
    assert_eq!(unsupported, PasskeyError::UnsupportedAlgorithm);

    let existing = [&result.credential];
    let excluded = [result.credential.credential_id()];
    let excluded_error = PasskeyAuthenticator::create_ctap(&CtapRegistrationRequest {
        existing_credentials: &existing,
        exclude_credential_ids: &excluded,
        ..ctap_registration_request(&[-7])
    })
    .err()
    .expect("excludeList should reject an existing credential");
    assert_eq!(excluded_error, PasskeyError::CredentialExcluded);
}

#[test]
fn ctap_assertion_reports_user_verification_in_flags() {
    let result = PasskeyAuthenticator::create_ctap(&CtapRegistrationRequest {
        user_verification: UserVerification::NotVerified,
        ..ctap_registration_request(&[-7])
    })
    .unwrap();
    assert_eq!(result.response.authenticator_data[32], 0x59);

    let assertion = result
        .credential
        .authenticate_ctap(&CtapAuthenticationRequest {
            client_data_hash: CTAP_CLIENT_DATA_HASH,
            rp_id: "example.com",
            allowed_credential_ids: &[],
            user_verification: UserVerification::NotVerified,
        })
        .unwrap();
    assert_eq!(assertion.authenticator_data[32], 0x19);
}

#[test]
fn validates_client_data_rp_and_credential_selection() {
    let result = PasskeyAuthenticator::create(&registration_request(&[-7])).unwrap();
    let wrong_id = [b"wrong".as_slice()];

    assert_eq!(
        result
            .credential
            .authenticate(&AuthenticationRequest {
                client_data_json: GET_CLIENT_DATA,
                origin: "https://login.example.com",
                challenge: GET_CHALLENGE,
                rp_id: "example.com",
                allowed_credential_ids: &wrong_id,
                user_verification: UserVerification::NotVerified,
            })
            .unwrap_err(),
        PasskeyError::CredentialNotAllowed
    );

    let wrong_origin = br#"{"type":"webauthn.get","challenge":"EBESExQVFhcYGRobHB0eHw","origin":"https://example.net","crossOrigin":false}"#;
    assert_eq!(
        result
            .credential
            .authenticate(&AuthenticationRequest {
                client_data_json: wrong_origin,
                origin: "https://login.example.com",
                challenge: GET_CHALLENGE,
                rp_id: "example.com",
                allowed_credential_ids: &[],
                user_verification: UserVerification::NotVerified,
            })
            .unwrap_err(),
        PasskeyError::OriginMismatch
    );

    let cross_origin = br#"{"type":"webauthn.get","challenge":"EBESExQVFhcYGRobHB0eHw","origin":"https://login.example.com","crossOrigin":true}"#;
    assert_eq!(
        result
            .credential
            .authenticate(&AuthenticationRequest {
                client_data_json: cross_origin,
                origin: "https://login.example.com",
                challenge: GET_CHALLENGE,
                rp_id: "example.com",
                allowed_credential_ids: &[],
                user_verification: UserVerification::NotVerified,
            })
            .unwrap_err(),
        PasskeyError::CrossOriginNotSupported
    );

    let public_suffix_error = PasskeyAuthenticator::create(&RegistrationRequest {
        algorithms: &[-7],
        rp_id: "com",
        ..registration_request(&[-7])
    })
    .err()
    .expect("public suffix should be rejected");
    assert_eq!(public_suffix_error, PasskeyError::InvalidRpId);

    let localhost = br#"{"type":"webauthn.create","challenge":"AAECAwQFBgcICQoLDA0ODw","origin":"http://localhost:3000","crossOrigin":false}"#;
    PasskeyAuthenticator::create(&RegistrationRequest {
        client_data_json: localhost,
        origin: "http://localhost:3000",
        rp_id: "localhost",
        ..registration_request(&[-7])
    })
    .expect("localhost HTTP origin should be accepted");

    for invalid_rp_id in ["example.com\\path", "127.0.0.1"] {
        let error = PasskeyAuthenticator::create(&RegistrationRequest {
            rp_id: invalid_rp_id,
            ..registration_request(&[-7])
        })
        .err()
        .expect("invalid RP IDs should be rejected");
        assert_eq!(error, PasskeyError::InvalidRpId);
    }

    let mismatched_challenge = result
        .credential
        .authenticate(&AuthenticationRequest {
            client_data_json: GET_CLIENT_DATA,
            origin: "https://login.example.com",
            challenge: CREATE_CHALLENGE,
            rp_id: "example.com",
            allowed_credential_ids: &[],
            user_verification: UserVerification::NotVerified,
        })
        .unwrap_err();
    assert_eq!(mismatched_challenge, PasskeyError::ChallengeMismatch);

    let existing = [&result.credential];
    let excluded = [result.credential.credential_id()];
    let excluded_error = PasskeyAuthenticator::create(&RegistrationRequest {
        existing_credentials: &existing,
        exclude_credential_ids: &excluded,
        ..registration_request(&[-7])
    })
    .err()
    .expect("excludeCredentials should reject an existing credential");
    assert_eq!(excluded_error, PasskeyError::CredentialExcluded);
}

#[test]
fn stores_protected_kpex_fields_and_roundtrips_xml() {
    let result = PasskeyAuthenticator::create(&registration_request(&[-8])).unwrap();
    let entry_id = NodeId::new_uuid();
    let mut entry = Entry::new(entry_id);
    result.credential.store_in_entry(&mut entry).unwrap();

    for name in [
        FIELD_CREDENTIAL_ID,
        FIELD_PRIVATE_KEY_PEM,
        FIELD_USER_HANDLE,
    ] {
        let (_, field) = entry
            .custom_fields()
            .find(|(_, field)| field.name == name)
            .unwrap();
        assert!(field.value.is_protected());
    }
    assert_eq!(
        result.credential.store_in_entry(&mut entry).unwrap_err(),
        PasskeyError::PasskeyAlreadyExists
    );

    let root_id = NodeId::new_uuid();
    let mut root = Group::new(root_id);
    root.title = "Root".to_string();
    root.add_child_entry(entry_id);
    let mut database = Database::new(DatabaseVersion::KDBX4);
    database.root_group_id = Some(root_id);
    database.groups.insert(root_id, root);
    database.entries.insert(entry_id, entry);

    let stream_key = b"passkey-xml-roundtrip";
    let mut write_stream = Salsa20InnerStream::new(stream_key).unwrap();
    let xml = KdbxXmlWriter::write(&database, &mut write_stream).unwrap();
    assert!(!xml.contains("BEGIN PRIVATE KEY"));

    let mut read_stream = Salsa20InnerStream::new(stream_key).unwrap();
    let loaded = KdbxXmlReader::read(&xml, &mut read_stream).unwrap();
    let loaded_entry = loaded.entries.get(&entry_id).unwrap();
    let loaded_credential = PasskeyCredential::from_entry(loaded_entry)
        .unwrap()
        .expect("passkey should survive XML roundtrip");
    assert_eq!(loaded_credential.algorithm(), PasskeyAlgorithm::Ed25519);
    assert_eq!(
        loaded_credential.credential_id(),
        result.credential.credential_id()
    );
    assert_eq!(loaded_credential.user_handle(), b"user-handle");
}

#[test]
fn field_values_match_stored_entry_fields() {
    let result = PasskeyAuthenticator::create_ctap(&ctap_registration_request(&[-7])).unwrap();
    let entry = entry_with_credential(&result.credential);

    for field in result.credential.to_field_values().unwrap() {
        let (_, stored) = entry
            .custom_fields()
            .find(|(_, stored)| stored.name == field.name)
            .unwrap_or_else(|| panic!("entry should contain {}", field.name));
        assert_eq!(stored.value.is_protected(), field.protected);
        assert_eq!(stored.value.as_str(), field.value.as_str());
    }

    assert_eq!(
        result.credential.to_field_values().unwrap().len(),
        entry.custom_fields().count()
    );
}

#[test]
fn reads_compatibility_fields_and_rejects_malformed_entries() {
    let result = PasskeyAuthenticator::create(&registration_request(&[-7])).unwrap();
    let mut entry = Entry::new(NodeId::new_uuid());
    result.credential.store_in_entry(&mut entry).unwrap();

    let credential_id = URL_SAFE_NO_PAD.encode(result.credential.credential_id());
    entry.retain_custom_fields(|field| field.name != FIELD_CREDENTIAL_ID);
    entry.add_custom_field(
        FIELD_GENERATED_USER_ID,
        ProtectedString::new_protected(&credential_id),
    );
    entry.add_custom_field(
        FIELD_COMPATIBLE_USERNAME,
        ProtectedString::new_plain("compatible-alice"),
    );

    let parsed = PasskeyCredential::from_entry(&entry).unwrap().unwrap();
    assert_eq!(parsed.username(), "compatible-alice");
    assert_eq!(parsed.credential_id(), result.credential.credential_id());

    entry.add_custom_field(FIELD_USER_HANDLE, ProtectedString::new_protected("AQID"));
    let duplicate_error = PasskeyCredential::from_entry(&entry)
        .err()
        .expect("duplicate fields should be rejected");
    assert_eq!(
        duplicate_error,
        PasskeyError::DuplicateField(FIELD_USER_HANDLE)
    );
}

#[test]
fn finds_credentials_by_relying_party_and_skips_malformed_entries() {
    let first = PasskeyAuthenticator::create_ctap(&ctap_registration_request(&[-7])).unwrap();
    let second = PasskeyAuthenticator::create_ctap(&CtapRegistrationRequest {
        username: "bob",
        ..ctap_registration_request(&[-8])
    })
    .unwrap();
    let other_rp = PasskeyAuthenticator::create_ctap(&CtapRegistrationRequest {
        rp_id: "example.net",
        ..ctap_registration_request(&[-7])
    })
    .unwrap();

    let mut malformed = Entry::new(NodeId::new_uuid());
    malformed.add_custom_field(
        FIELD_CREDENTIAL_ID,
        ProtectedString::new_protected("not-a-passkey"),
    );
    let plain_entry = Entry::new(NodeId::new_uuid());

    let (database, composite_key) = database_with_entries(vec![
        entry_with_credential(&first.credential),
        entry_with_credential(&second.credential),
        entry_with_credential(&other_rp.credential),
        malformed,
        plain_entry,
    ]);

    let all = find_credentials(&database, &composite_key, None).unwrap();
    assert_eq!(all.len(), 3);

    let filtered = find_credentials(&database, &composite_key, Some("EXAMPLE.com")).unwrap();
    let mut expected = vec![
        first.credential.credential_id().to_vec(),
        second.credential.credential_id().to_vec(),
    ];
    expected.sort();
    assert_eq!(
        filtered
            .iter()
            .map(|(_, credential)| credential.credential_id().to_vec())
            .collect::<Vec<_>>(),
        expected
    );
    for (entry_id, credential) in &filtered {
        assert_eq!(credential.rp_id(), "example.com");
        assert!(database.entries.contains_key(entry_id));
    }

    assert!(
        find_credentials(&database, &composite_key, Some("missing.example"))
            .unwrap()
            .is_empty()
    );
    let invalid_filter = find_credentials(&database, &composite_key, Some("not a domain"))
        .err()
        .expect("invalid RP ID filters should be rejected");
    assert_eq!(invalid_filter, PasskeyError::InvalidRpId);
}
