use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use keeless_kdbx::model::core::node::NodeId;
use keeless_kdbx::model::db::database::{Database, DatabaseVersion};
use keeless_kdbx::model::entry::passkey::{
    AuthenticationRequest, PasskeyAlgorithm, PasskeyAuthenticator, PasskeyCredential, PasskeyError,
    RegistrationRequest, UserVerification, FIELD_COMPATIBLE_USERNAME, FIELD_CREDENTIAL_ID,
    FIELD_GENERATED_USER_ID, FIELD_PRIVATE_KEY_PEM, FIELD_USER_HANDLE, KEELESS_AAGUID,
};
use keeless_kdbx::model::entry::{Entry, EntryField};
use keeless_kdbx::model::group::Group;
use keeless_kdbx::{KdbxXmlReader, KdbxXmlWriter, ProtectedString, Salsa20InnerStream};
use p256::ecdsa::signature::Verifier;
use p256::pkcs8::DecodePublicKey;
use sha2::{Digest, Sha256};

const CREATE_CHALLENGE: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const GET_CHALLENGE: &[u8] = &[
    16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
];
const CREATE_CLIENT_DATA: &[u8] = br#"{"type":"webauthn.create","challenge":"AAECAwQFBgcICQoLDA0ODw","origin":"https://login.example.com","crossOrigin":false}"#;
const GET_CLIENT_DATA: &[u8] = br#"{"type":"webauthn.get","challenge":"EBESExQVFhcYGRobHB0eHw","origin":"https://login.example.com","crossOrigin":false}"#;

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

fn signed_message(authenticator_data: &[u8], client_data_json: &[u8]) -> Vec<u8> {
    let mut message = authenticator_data.to_vec();
    message.extend_from_slice(&Sha256::digest(client_data_json));
    message
}

#[test]
fn creates_none_attestation_and_authenticates_with_all_algorithms() {
    for algorithm in [
        PasskeyAlgorithm::Es256,
        PasskeyAlgorithm::Rs256,
        PasskeyAlgorithm::Ed25519,
    ] {
        let result =
            PasskeyAuthenticator::create(&registration_request(&[12345, algorithm.cose_id()]))
                .expect("registration should succeed");

        assert_eq!(result.credential.algorithm(), algorithm);
        assert_eq!(result.response.public_key_algorithm, algorithm.cose_id());
        assert_eq!(result.response.client_data_json, CREATE_CLIENT_DATA);
        assert_eq!(result.response.authenticator_data[32], 0x5d);
        assert_eq!(&result.response.authenticator_data[37..53], &KEELESS_AAGUID);
        assert_eq!(
            u16::from_be_bytes([
                result.response.authenticator_data[53],
                result.response.authenticator_data[54],
            ]) as usize,
            result.response.credential_id.len()
        );

        let cose_offset = 55 + result.response.credential_id.len();
        let mut cose = minicbor::Decoder::new(&result.response.authenticator_data[cose_offset..]);
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

        let message = signed_message(&assertion.authenticator_data, GET_CLIENT_DATA);
        match algorithm {
            PasskeyAlgorithm::Es256 => {
                let public = p256::PublicKey::from_public_key_der(&result.response.public_key_spki)
                    .expect("valid P-256 SPKI");
                let verifier = p256::ecdsa::VerifyingKey::from(public);
                let signature = p256::ecdsa::DerSignature::from_bytes(&assertion.signature)
                    .expect("valid DER signature");
                verifier.verify(&message, &signature).unwrap();
            }
            PasskeyAlgorithm::Rs256 => {
                let public =
                    rsa::RsaPublicKey::from_public_key_der(&result.response.public_key_spki)
                        .expect("valid RSA SPKI");
                let verifier = rsa::pkcs1v15::VerifyingKey::<Sha256>::new(public);
                let signature = rsa::pkcs1v15::Signature::try_from(assertion.signature.as_slice())
                    .expect("valid RSA signature");
                verifier.verify(&message, &signature).unwrap();
            }
            PasskeyAlgorithm::Ed25519 => {
                let verifier = ed25519_dalek::VerifyingKey::from_public_key_der(
                    &result.response.public_key_spki,
                )
                .expect("valid Ed25519 SPKI");
                let signature = ed25519_dalek::Signature::from_slice(&assertion.signature)
                    .expect("valid Ed25519 signature");
                verifier.verify(&message, &signature).unwrap();
            }
        }
    }
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
        let field = entry
            .custom_fields
            .iter()
            .find(|field| field.name == name)
            .unwrap();
        assert!(field.is_protected);
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
    let mut write_stream = Salsa20InnerStream::new(stream_key);
    let xml = KdbxXmlWriter::write(&database, &mut write_stream).unwrap();
    assert!(!xml.contains("BEGIN PRIVATE KEY"));

    let mut read_stream = Salsa20InnerStream::new(stream_key);
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
fn reads_compatibility_fields_and_rejects_malformed_entries() {
    let result = PasskeyAuthenticator::create(&registration_request(&[-7])).unwrap();
    let mut entry = Entry::new(NodeId::new_uuid());
    result.credential.store_in_entry(&mut entry).unwrap();

    let credential_id = URL_SAFE_NO_PAD.encode(result.credential.credential_id());
    entry
        .custom_fields
        .retain(|field| field.name != FIELD_CREDENTIAL_ID);
    entry.custom_fields.push(EntryField {
        name: FIELD_GENERATED_USER_ID.to_string(),
        value: ProtectedString::new_protected(&credential_id),
        is_protected: true,
    });
    entry.custom_fields.push(EntryField {
        name: FIELD_COMPATIBLE_USERNAME.to_string(),
        value: ProtectedString::new_plain("compatible-alice"),
        is_protected: false,
    });

    let parsed = PasskeyCredential::from_entry(&entry).unwrap().unwrap();
    assert_eq!(parsed.username(), "compatible-alice");
    assert_eq!(parsed.credential_id(), result.credential.credential_id());

    entry.custom_fields.push(EntryField {
        name: FIELD_USER_HANDLE.to_string(),
        value: ProtectedString::new_protected("AQID"),
        is_protected: true,
    });
    let duplicate_error = PasskeyCredential::from_entry(&entry)
        .err()
        .expect("duplicate fields should be rejected");
    assert_eq!(
        duplicate_error,
        PasskeyError::DuplicateField(FIELD_USER_HANDLE)
    );
}
