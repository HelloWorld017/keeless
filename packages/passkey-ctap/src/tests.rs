use minicbor::Encoder;

use crate::error::CtapStatus;
use crate::request::{Command, parse_command};
use crate::response::{self, Assertion, AuthenticatorInfo};

use crate::KEELESS_AAGUID as AAGUID;

const CLIENT_DATA_HASH: [u8; 32] = [7; 32];

/// Build a CTAP payload: the command byte followed by an encoded parameter map.
fn payload(command: u8, encode: impl FnOnce(&mut Encoder<Vec<u8>>)) -> Vec<u8> {
    let mut encoder = Encoder::new(vec![command]);
    encode(&mut encoder);
    encoder.into_writer()
}

/// `pubKeyCredParams` holding one entry per algorithm, all of type `public-key`.
fn credential_parameters(encoder: &mut Encoder<Vec<u8>>, algorithms: &[i32]) {
    encoder.array(algorithms.len() as u64).unwrap();
    for algorithm in algorithms {
        encoder.map(2).unwrap();
        encoder.str("alg").unwrap();
        encoder.i32(*algorithm).unwrap();
        encoder.str("type").unwrap();
        encoder.str("public-key").unwrap();
    }
}

fn credential_list(encoder: &mut Encoder<Vec<u8>>, ids: &[&[u8]]) {
    encoder.array(ids.len() as u64).unwrap();
    for id in ids {
        encoder.map(2).unwrap();
        encoder.str("id").unwrap();
        encoder.bytes(id).unwrap();
        encoder.str("type").unwrap();
        encoder.str("public-key").unwrap();
    }
}

/// A complete `makeCredential` as a platform sends it.
fn make_credential_payload() -> Vec<u8> {
    payload(0x01, |encoder| {
        encoder.map(5).unwrap();
        encoder.u8(0x01).unwrap();
        encoder.bytes(&CLIENT_DATA_HASH).unwrap();
        encoder.u8(0x02).unwrap();
        encoder.map(2).unwrap();
        encoder.str("id").unwrap();
        encoder.str("example.com").unwrap();
        encoder.str("name").unwrap();
        encoder.str("Example").unwrap();
        encoder.u8(0x03).unwrap();
        encoder.map(3).unwrap();
        encoder.str("id").unwrap();
        encoder.bytes(&[1, 2, 3, 4, 5]).unwrap();
        encoder.str("name").unwrap();
        encoder.str("alice").unwrap();
        encoder.str("displayName").unwrap();
        encoder.str("Alice").unwrap();
        encoder.u8(0x04).unwrap();
        credential_parameters(encoder, &[-7, -257]);
        encoder.u8(0x07).unwrap();
        encoder.map(2).unwrap();
        encoder.str("rk").unwrap();
        encoder.bool(true).unwrap();
        encoder.str("uv").unwrap();
        encoder.bool(true).unwrap();
    })
}

/// A `getAssertion` with the given parameter count, letting each test add its own.
fn get_assertion_payload(entries: u64, extra: impl FnOnce(&mut Encoder<Vec<u8>>)) -> Vec<u8> {
    payload(0x02, |encoder| {
        encoder.map(2 + entries).unwrap();
        encoder.u8(0x01).unwrap();
        encoder.str("example.com").unwrap();
        encoder.u8(0x02).unwrap();
        encoder.bytes(&CLIENT_DATA_HASH).unwrap();
        extra(encoder);
    })
}

#[test]
fn parses_a_make_credential_request() {
    let Command::MakeCredential(request) = parse_command(&make_credential_payload()).unwrap()
    else {
        panic!("expected makeCredential");
    };
    assert_eq!(request.client_data_hash, CLIENT_DATA_HASH);
    assert_eq!(request.rp_id, "example.com");
    assert_eq!(request.rp_name.as_deref(), Some("Example"));
    assert_eq!(request.user_id, vec![1, 2, 3, 4, 5]);
    assert_eq!(request.user_name, "alice");
    assert_eq!(request.user_display_name.as_deref(), Some("Alice"));
    assert_eq!(request.algorithms, vec![-7, -257]);
    assert!(request.exclude_credential_ids.is_empty());
}

#[test]
fn parses_an_exclude_list() {
    let payload = payload(0x01, |encoder| {
        encoder.map(5).unwrap();
        encoder.u8(0x01).unwrap();
        encoder.bytes(&CLIENT_DATA_HASH).unwrap();
        encoder.u8(0x02).unwrap();
        encoder.map(1).unwrap();
        encoder.str("id").unwrap();
        encoder.str("example.com").unwrap();
        encoder.u8(0x03).unwrap();
        encoder.map(2).unwrap();
        encoder.str("id").unwrap();
        encoder.bytes(&[9]).unwrap();
        encoder.str("name").unwrap();
        encoder.str("alice").unwrap();
        encoder.u8(0x04).unwrap();
        credential_parameters(encoder, &[-7]);
        encoder.u8(0x05).unwrap();
        credential_list(encoder, &[&[0xaa, 0xbb], &[0xcc]]);
    });

    let Command::MakeCredential(request) = parse_command(&payload).unwrap() else {
        panic!("expected makeCredential");
    };
    assert_eq!(
        request.exclude_credential_ids,
        vec![vec![0xaa, 0xbb], vec![0xcc]]
    );
}

#[test]
fn parses_a_get_assertion_request_with_an_allow_list() {
    let payload = get_assertion_payload(1, |encoder| {
        encoder.u8(0x03).unwrap();
        credential_list(encoder, &[&[0xaa, 0xbb, 0xcc]]);
    });

    let Command::GetAssertion(request) = parse_command(&payload).unwrap() else {
        panic!("expected getAssertion");
    };
    assert_eq!(request.rp_id, "example.com");
    assert_eq!(request.client_data_hash, CLIENT_DATA_HASH);
    assert_eq!(request.allow_credential_ids, vec![vec![0xaa, 0xbb, 0xcc]]);
}

#[test]
fn parses_parameterless_commands() {
    assert_eq!(parse_command(&[0x04]).unwrap(), Command::GetInfo);
    assert_eq!(parse_command(&[0x08]).unwrap(), Command::GetNextAssertion);
    assert_eq!(parse_command(&[0x0b]).unwrap(), Command::Selection);
    assert_eq!(parse_command(&[0x07]).unwrap(), Command::Reset);
}

#[test]
fn rejects_unknown_and_unimplemented_commands() {
    assert_eq!(
        parse_command(&[]).unwrap_err().status,
        CtapStatus::InvalidLength
    );
    assert_eq!(
        parse_command(&[0x99]).unwrap_err().status,
        CtapStatus::InvalidCommand
    );
    // clientPIN, which this authenticator does not implement.
    assert_eq!(
        parse_command(&[0x06]).unwrap_err().status,
        CtapStatus::InvalidCommand
    );
}

#[test]
fn rejects_a_make_credential_missing_required_parameters() {
    let payload = payload(0x01, |encoder| {
        encoder.map(3).unwrap();
        encoder.u8(0x01).unwrap();
        encoder.bytes(&CLIENT_DATA_HASH).unwrap();
        encoder.u8(0x02).unwrap();
        encoder.map(1).unwrap();
        encoder.str("id").unwrap();
        encoder.str("example.com").unwrap();
        encoder.u8(0x03).unwrap();
        encoder.map(1).unwrap();
        encoder.str("id").unwrap();
        encoder.bytes(&[1]).unwrap();
    });
    assert_eq!(
        parse_command(&payload).unwrap_err().status,
        CtapStatus::MissingParameter
    );
}

#[test]
fn accepts_a_silent_get_assertion_but_never_a_silent_registration() {
    // Platforms probe with up=false to learn which credentials exist before
    // deciding whether to prompt; refusing it makes every such sign-in look
    // like the authenticator held nothing.
    let silent = get_assertion_payload(1, |encoder| {
        encoder.u8(0x05).unwrap();
        encoder.map(1).unwrap();
        encoder.str("up").unwrap();
        encoder.bool(false).unwrap();
    });
    let Command::GetAssertion(request) = parse_command(&silent).unwrap() else {
        panic!("expected getAssertion");
    };
    assert!(!request.user_presence);

    // Absent or true means the user is asked.
    let Command::GetAssertion(request) = parse_command(&get_assertion_payload(0, |_| {})).unwrap()
    else {
        panic!("expected getAssertion");
    };
    assert!(request.user_presence);

    let silent_registration = payload(0x01, |encoder| {
        encoder.map(5).unwrap();
        encoder.u8(0x01).unwrap();
        encoder.bytes(&CLIENT_DATA_HASH).unwrap();
        encoder.u8(0x02).unwrap();
        encoder.map(1).unwrap();
        encoder.str("id").unwrap();
        encoder.str("example.com").unwrap();
        encoder.u8(0x03).unwrap();
        encoder.map(1).unwrap();
        encoder.str("id").unwrap();
        encoder.bytes(&[1]).unwrap();
        encoder.u8(0x04).unwrap();
        credential_parameters(encoder, &[-7]);
        encoder.u8(0x07).unwrap();
        encoder.map(1).unwrap();
        encoder.str("up").unwrap();
        encoder.bool(false).unwrap();
    });
    assert_eq!(
        parse_command(&silent_registration).unwrap_err().status,
        CtapStatus::InvalidOption
    );
}

#[test]
fn rejects_pin_authenticated_requests_by_what_they_are_missing() {
    // A token naming a protocol: every protocol is unsupported here.
    let versioned = get_assertion_payload(2, |encoder| {
        encoder.u8(0x06).unwrap();
        encoder.bytes(&[0xaa, 0xbb]).unwrap();
        encoder.u8(0x07).unwrap();
        encoder.u8(1).unwrap();
    });
    assert_eq!(
        parse_command(&versioned).unwrap_err().status,
        CtapStatus::InvalidParameter
    );

    // A token naming none is incomplete rather than unsupported.
    let unversioned = get_assertion_payload(1, |encoder| {
        encoder.u8(0x06).unwrap();
        encoder.bytes(&[0xaa, 0xbb]).unwrap();
    });
    assert_eq!(
        parse_command(&unversioned).unwrap_err().status,
        CtapStatus::MissingParameter
    );

    // A protocol number with no token is meaningless, not an error.
    let bare = get_assertion_payload(1, |encoder| {
        encoder.u8(0x07).unwrap();
        encoder.u8(1).unwrap();
    });
    assert!(matches!(
        parse_command(&bare).unwrap(),
        Command::GetAssertion(_)
    ));
}

#[test]
fn rejects_malformed_and_indefinite_cbor() {
    let truncated = &make_credential_payload()[..6];
    assert_eq!(
        parse_command(truncated).unwrap_err().status,
        CtapStatus::InvalidCbor
    );

    // An indefinite-length map, which CTAP2 canonical CBOR forbids.
    assert_eq!(
        parse_command(&[0x02, 0xbf, 0xff]).unwrap_err().status,
        CtapStatus::InvalidCbor
    );

    // rpId sent as bytes rather than text.
    let mistyped = payload(0x02, |encoder| {
        encoder.map(2).unwrap();
        encoder.u8(0x01).unwrap();
        encoder.bytes(b"example.com").unwrap();
        encoder.u8(0x02).unwrap();
        encoder.bytes(&CLIENT_DATA_HASH).unwrap();
    });
    assert_eq!(
        parse_command(&mistyped).unwrap_err().status,
        CtapStatus::CborUnexpectedType
    );
}

#[test]
fn ignores_unknown_parameters_and_non_public_key_entries() {
    let payload = payload(0x01, |encoder| {
        encoder.map(6).unwrap();
        encoder.u8(0x01).unwrap();
        encoder.bytes(&CLIENT_DATA_HASH).unwrap();
        encoder.u8(0x02).unwrap();
        encoder.map(1).unwrap();
        encoder.str("id").unwrap();
        encoder.str("example.com").unwrap();
        encoder.u8(0x03).unwrap();
        encoder.map(1).unwrap();
        encoder.str("id").unwrap();
        encoder.bytes(&[1, 2]).unwrap();
        encoder.u8(0x04).unwrap();
        // An entry of an unknown credential type must not contribute its algorithm.
        encoder.array(2).unwrap();
        encoder.map(2).unwrap();
        encoder.str("alg").unwrap();
        encoder.i32(-7).unwrap();
        encoder.str("type").unwrap();
        encoder.str("future-key").unwrap();
        encoder.map(2).unwrap();
        encoder.str("alg").unwrap();
        encoder.i32(-8).unwrap();
        encoder.str("type").unwrap();
        encoder.str("public-key").unwrap();
        // Extensions we do not implement.
        encoder.u8(0x06).unwrap();
        encoder.map(1).unwrap();
        encoder.str("hmac-secret").unwrap();
        encoder.bool(true).unwrap();
        // A PIN protocol number without a token.
        encoder.u8(0x09).unwrap();
        encoder.u8(1).unwrap();
    });

    let Command::MakeCredential(request) = parse_command(&payload).unwrap() else {
        panic!("expected makeCredential");
    };
    assert_eq!(request.algorithms, vec![-8]);
    assert_eq!(request.rp_name, None);
    // No name and no displayName still yields something the entry can show.
    assert_eq!(request.user_name, "unknown");
    assert_eq!(request.user_display_name, None);
}

#[test]
fn falls_back_to_the_display_name_when_no_name_is_sent() {
    let payload = payload(0x01, |encoder| {
        encoder.map(4).unwrap();
        encoder.u8(0x01).unwrap();
        encoder.bytes(&CLIENT_DATA_HASH).unwrap();
        encoder.u8(0x02).unwrap();
        encoder.map(1).unwrap();
        encoder.str("id").unwrap();
        encoder.str("example.com").unwrap();
        encoder.u8(0x03).unwrap();
        encoder.map(2).unwrap();
        encoder.str("id").unwrap();
        encoder.bytes(&[1]).unwrap();
        encoder.str("displayName").unwrap();
        encoder.str("Alice Example").unwrap();
        encoder.u8(0x04).unwrap();
        credential_parameters(encoder, &[-7]);
    });

    let Command::MakeCredential(request) = parse_command(&payload).unwrap() else {
        panic!("expected makeCredential");
    };
    assert_eq!(request.user_name, "Alice Example");
}

#[test]
fn encodes_a_make_credential_response() {
    let authenticator_data = [0x42; 37];
    let encoded = response::make_credential(&authenticator_data).unwrap();
    assert_eq!(encoded[0], 0x00);

    let mut decoder = minicbor::Decoder::new(&encoded[1..]);
    assert_eq!(decoder.map().unwrap(), Some(3));
    assert_eq!(decoder.u8().unwrap(), 0x01);
    assert_eq!(decoder.str().unwrap(), "none");
    assert_eq!(decoder.u8().unwrap(), 0x02);
    assert_eq!(decoder.bytes().unwrap(), authenticator_data);
    assert_eq!(decoder.u8().unwrap(), 0x03);
    assert_eq!(decoder.map().unwrap(), Some(0));
    assert_eq!(decoder.position(), encoded.len() - 1);
}

#[test]
fn encodes_a_get_assertion_response() {
    let encoded = response::get_assertion(&Assertion {
        credential_id: &[1, 2, 3],
        authenticator_data: &[0x42; 37],
        signature: &[9, 9, 9],
        user_id: &[4, 5],
        user_name: Some("alice"),
        user_selected: false,
    })
    .unwrap();
    assert_eq!(encoded[0], 0x00);

    let mut decoder = minicbor::Decoder::new(&encoded[1..]);
    assert_eq!(decoder.map().unwrap(), Some(4));
    assert_eq!(decoder.u8().unwrap(), 0x01);
    assert_eq!(decoder.map().unwrap(), Some(2));
    assert_eq!(decoder.str().unwrap(), "id");
    assert_eq!(decoder.bytes().unwrap(), &[1, 2, 3]);
    assert_eq!(decoder.str().unwrap(), "type");
    assert_eq!(decoder.str().unwrap(), "public-key");
    assert_eq!(decoder.u8().unwrap(), 0x02);
    assert_eq!(decoder.bytes().unwrap().len(), 37);
    assert_eq!(decoder.u8().unwrap(), 0x03);
    assert_eq!(decoder.bytes().unwrap(), &[9, 9, 9]);
    assert_eq!(decoder.u8().unwrap(), 0x04);
    assert_eq!(decoder.map().unwrap(), Some(2));
    assert_eq!(decoder.str().unwrap(), "id");
    assert_eq!(decoder.bytes().unwrap(), &[4, 5]);
    assert_eq!(decoder.str().unwrap(), "name");
    assert_eq!(decoder.str().unwrap(), "alice");
    assert_eq!(decoder.position(), encoded.len() - 1);
}

#[test]
fn a_silent_assertion_names_nobody_and_claims_no_selection() {
    let encoded = response::get_assertion(&Assertion {
        credential_id: &[1],
        authenticator_data: &[0x42; 37],
        signature: &[9],
        user_id: &[4, 5],
        user_name: None,
        user_selected: false,
    })
    .unwrap();

    let mut decoder = minicbor::Decoder::new(&encoded[1..]);
    assert_eq!(decoder.map().unwrap(), Some(4));
    for _ in 0..3 {
        decoder.u8().unwrap();
        decoder.skip().unwrap();
    }
    assert_eq!(decoder.u8().unwrap(), 0x04);
    assert_eq!(decoder.map().unwrap(), Some(1), "only the user handle");
    assert_eq!(decoder.str().unwrap(), "id");
    assert_eq!(decoder.bytes().unwrap(), &[4, 5]);
    assert_eq!(decoder.position(), encoded.len() - 1);
}

#[test]
fn an_authenticator_chosen_credential_reports_user_selected() {
    let encoded = response::get_assertion(&Assertion {
        credential_id: &[1],
        authenticator_data: &[0x42; 37],
        signature: &[9],
        user_id: &[4],
        user_name: Some("alice"),
        user_selected: true,
    })
    .unwrap();

    let mut decoder = minicbor::Decoder::new(&encoded[1..]);
    assert_eq!(decoder.map().unwrap(), Some(5));
    for _ in 0..4 {
        decoder.u8().unwrap();
        decoder.skip().unwrap();
    }
    assert_eq!(decoder.u8().unwrap(), 0x06);
    assert!(decoder.bool().unwrap());
    assert_eq!(decoder.position(), encoded.len() - 1);
}

#[test]
fn encodes_get_info_without_a_client_pin_option() {
    let info = AuthenticatorInfo {
        aaguid: &AAGUID,
        platform_device: false,
        transports: &["usb"],
    };
    let encoded = response::get_info(&info).unwrap();
    assert_eq!(encoded[0], 0x00);

    let mut decoder = minicbor::Decoder::new(&encoded[1..]);
    assert_eq!(decoder.map().unwrap(), Some(8));
    assert_eq!(decoder.u8().unwrap(), 0x01);
    assert_eq!(
        decoder.array().unwrap(),
        Some(1),
        "only the version whose mandatory features are implemented"
    );
    assert_eq!(decoder.str().unwrap(), "FIDO_2_0");
    assert_eq!(decoder.u8().unwrap(), 0x03);
    assert_eq!(decoder.bytes().unwrap(), AAGUID);
    assert_eq!(decoder.u8().unwrap(), 0x04);
    assert_eq!(decoder.map().unwrap(), Some(4));
    for expected in ["rk", "up", "uv"] {
        assert_eq!(decoder.str().unwrap(), expected);
        assert!(decoder.bool().unwrap());
    }
    assert_eq!(decoder.str().unwrap(), "plat");
    assert!(!decoder.bool().unwrap());
    assert_eq!(decoder.u8().unwrap(), 0x05);
    assert_eq!(decoder.u32().unwrap(), response::MAX_MESSAGE_SIZE);
    assert_eq!(decoder.u8().unwrap(), 0x07);
    assert_eq!(
        decoder.u32().unwrap(),
        response::MAX_CREDENTIAL_COUNT_IN_LIST
    );
    assert_eq!(decoder.u8().unwrap(), 0x08);
    assert_eq!(decoder.u32().unwrap(), response::MAX_CREDENTIAL_ID_LENGTH);
    assert_eq!(decoder.u8().unwrap(), 0x09);
    assert_eq!(decoder.array().unwrap(), Some(1));
    assert_eq!(decoder.str().unwrap(), "usb");
    assert_eq!(decoder.u8().unwrap(), 0x0a);
    assert_eq!(
        decoder.array().unwrap(),
        Some(response::SUPPORTED_ALGORITHMS.len() as u64)
    );
    for algorithm in response::SUPPORTED_ALGORITHMS {
        assert_eq!(decoder.map().unwrap(), Some(2));
        assert_eq!(decoder.str().unwrap(), "alg");
        assert_eq!(decoder.i32().unwrap(), algorithm);
        assert_eq!(decoder.str().unwrap(), "type");
        assert_eq!(decoder.str().unwrap(), "public-key");
    }
    assert_eq!(decoder.position(), encoded.len() - 1);

    let raw = response::authenticator_info_cbor(&info).unwrap();
    assert_eq!(raw, encoded[1..]);
}

#[test]
fn encodes_a_bare_status_byte() {
    assert_eq!(
        response::status(CtapStatus::NoCredentials),
        vec![CtapStatus::NoCredentials.as_u8()]
    );
}
