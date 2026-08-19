use std::{
    ffi::OsString,
    sync::{Arc, Mutex},
};

use keeless_lesswire::{
    ApprovalProvider, Identity, KeyScope, Server, ServerHost, StateStore, SystemClock, WireFuture,
};

use crate::{
    Error, encrypt,
    protocol::{Arguments, DialogAnchor, PlaintextResponse, UiRequest},
    secure_text_edit::SecureTextBuffer,
};

#[test]
fn parses_each_ui_kind() {
    let identity = Identity::generate(KeyScope::CoreUntrusted).unwrap();
    let key = identity.public_key_bundle();
    let connection = format!(
        r#"{{"publicKey":"{key}","senderScope":"core_untrusted","recipient":"{key}","recipientScope":"core_untrusted","kind":"initial"}}"#
    );
    for (kind, json) in [
        ("password", r#"{"mode":"unlock"}"#),
        ("password", r#"{"mode":"session"}"#),
        ("connection", connection.as_str()),
        (
            "passkey",
            r#"{"mode":"assert","rpId":"example.com","accounts":[{"id":"1","username":"alice"}]}"#,
        ),
    ] {
        let arguments = Arguments::parse(arguments(&key, kind, json)).unwrap();
        assert!(matches!(
            (kind, arguments.request),
            ("password", UiRequest::Password(_))
                | ("connection", UiRequest::Connection(_))
                | ("passkey", UiRequest::Passkey(_))
        ));
    }
}

#[test]
fn parses_a_physical_screen_anchor() {
    let key = Identity::generate(KeyScope::CoreUntrusted)
        .unwrap()
        .public_key_bundle();
    let arguments = Arguments::parse(
        [
            "--public-key",
            key.as_str(),
            "--anchor",
            "-1440",
            "900",
            "password",
            r#"{"mode":"unlock"}"#,
        ]
        .into_iter()
        .map(OsString::from),
    )
    .unwrap();

    assert_eq!(arguments.anchor, Some(DialogAnchor { x: -1440, y: 900 }));
}

#[test]
fn rejects_invalid_screen_anchors() {
    let key = Identity::generate(KeyScope::CoreUntrusted)
        .unwrap()
        .public_key_bundle();
    let invalid_x = Arguments::parse(
        [
            "--public-key",
            key.as_str(),
            "--anchor",
            "x",
            "0",
            "password",
            r#"{"mode":"unlock"}"#,
        ]
        .into_iter()
        .map(OsString::from),
    );
    let missing_y = Arguments::parse(
        ["--public-key", key.as_str(), "--anchor", "0"]
            .into_iter()
            .map(OsString::from),
    );

    assert!(matches!(invalid_x, Err(Error::Usage(_))));
    assert!(matches!(missing_y, Err(Error::Usage(_))));
}

#[test]
fn rejects_unknown_fields_and_unsafe_labels() {
    let key = Identity::generate(KeyScope::CoreUntrusted)
        .unwrap()
        .public_key_bundle();
    let unknown = Arguments::parse(arguments(
        &key,
        "password",
        r#"{"mode":"unlock","secret":"value"}"#,
    ));
    assert!(matches!(unknown, Err(Error::InvalidRequest(_))));

    let bidi_name = Arguments::parse(arguments(
        &key,
        "connection",
        &format!(
            r#"{{"publicKey":"{key}","senderScope":"bad","recipient":"{key}","recipientScope":"core_untrusted","kind":"initial"}}"#
        ),
    ));
    assert!(matches!(bidi_name, Err(Error::InvalidRequest(_))));

    let bidi_rp = Arguments::parse(arguments(
        &key,
        "passkey",
        r#"{"mode":"assert","rpId":"safe\u202eevil.com","accounts":[{"id":"1","username":"a"}]}"#,
    ));
    assert!(matches!(bidi_rp, Err(Error::InvalidRequest(_))));
}

#[test]
fn rejects_passkey_account_counts_the_dialog_cannot_show() {
    let key = Identity::generate(KeyScope::CoreUntrusted)
        .unwrap()
        .public_key_bundle();
    for json in [
        // A registration prompt names exactly the account being created.
        r#"{"mode":"register","rpId":"example.com","accounts":[]}"#,
        r#"{"mode":"register","rpId":"example.com","accounts":[{"id":"1","username":"a"},{"id":"2","username":"b"}]}"#,
        // A sign-in prompt needs at least one account to choose from.
        r#"{"mode":"assert","rpId":"example.com","accounts":[]}"#,
    ] {
        assert!(matches!(
            Arguments::parse(arguments(&key, "passkey", json)),
            Err(Error::InvalidRequest(_))
        ));
    }

    let many = (0..33)
        .map(|index| format!(r#"{{"id":"{index}","username":"user{index}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    assert!(matches!(
        Arguments::parse(arguments(
            &key,
            "passkey",
            &format!(r#"{{"mode":"assert","rpId":"example.com","accounts":[{many}]}}"#),
        )),
        Err(Error::InvalidRequest(_))
    ));
}

#[test]
fn serializes_cancelled_and_denied_results() {
    assert_eq!(
        String::from_utf8(
            PlaintextResponse::password(None)
                .to_json()
                .unwrap()
                .to_vec()
        )
        .unwrap(),
        r#"{"version":1,"kind":"password","status":"cancelled"}"#
    );
    assert_eq!(
        String::from_utf8(
            PlaintextResponse::connection(Some(false))
                .to_json()
                .unwrap()
                .to_vec(),
        )
        .unwrap(),
        r#"{"version":1,"kind":"connection","status":"selected","result":{"allowed":false}}"#
    );
    assert_eq!(
        String::from_utf8(PlaintextResponse::passkey(None).to_json().unwrap().to_vec()).unwrap(),
        r#"{"version":1,"kind":"passkey","status":"cancelled"}"#
    );
    assert_eq!(
        String::from_utf8(
            PlaintextResponse::passkey(Some("entry-1".into()))
                .to_json()
                .unwrap()
                .to_vec()
        )
        .unwrap(),
        r#"{"version":1,"kind":"passkey","status":"selected","result":{"accountId":"entry-1"}}"#
    );
}

#[test]
fn serializes_selected_password_as_json() {
    let password = SecureTextBuffer::from_text("s\"ecret한").unwrap();
    let json = PlaintextResponse::password(Some(password))
        .to_json()
        .unwrap();
    assert_eq!(
        String::from_utf8(json.to_vec()).unwrap(),
        r#"{"version":1,"kind":"password","status":"selected","result":{"password":"s\"ecret한"}}"#
    );
}

#[tokio::test]
async fn encrypted_response_round_trips_through_lesswire_server() {
    let store = Arc::new(MemoryStore::default());
    let mut server = Server::new(ServerHost {
        store,
        approval_provider: Arc::new(DenyApproval),
        clock: Arc::new(SystemClock),
        scope: KeyScope::CoreUntrusted,
        allow_transfers: false,
        runtime_approved_clients: Vec::new(),
    })
    .await
    .unwrap();
    let recipient = keeless_lesswire::PublicKeyBundle::parse(&server.public_key_bundle()).unwrap();
    let plaintext = br#"{"version":1,"kind":"password","status":"cancelled"}"#;
    let frame = encrypt(&recipient, plaintext).unwrap();
    assert_eq!(
        keeless_lesswire::PublicKeyBundle::parse(&frame.public_key)
            .unwrap()
            .scope,
        KeyScope::NativeUi
    );
    server.add_runtime_approval(&frame.public_key).unwrap();

    let observed = Arc::new(Mutex::new(Vec::new()));
    let captured = observed.clone();
    let response = server
        .handle_frame(&frame, move |_owner, value| {
            *captured.lock().unwrap() = value.to_vec();
            async { Ok::<Option<Vec<u8>>, ()>(None) }
        })
        .await
        .unwrap();
    assert!(response.is_none());
    assert_eq!(&*observed.lock().unwrap(), plaintext);
}

fn arguments(key: &str, kind: &str, json: &str) -> Vec<OsString> {
    ["--public-key", key, kind, json]
        .into_iter()
        .map(OsString::from)
        .collect()
}

#[derive(Default)]
struct MemoryStore(Mutex<Option<Vec<u8>>>);

impl StateStore for MemoryStore {
    fn load(&self) -> WireFuture<'_, keeless_lesswire::Result<Option<Vec<u8>>>> {
        Box::pin(async { Ok(self.0.lock().unwrap().clone()) })
    }

    fn save<'a>(&'a self, value: &'a [u8]) -> WireFuture<'a, keeless_lesswire::Result<()>> {
        Box::pin(async move {
            *self.0.lock().unwrap() = Some(value.to_vec());
            Ok(())
        })
    }
}

struct DenyApproval;

impl ApprovalProvider for DenyApproval {
    fn approve(
        &self,
        _: keeless_lesswire::ApprovalRequest,
    ) -> WireFuture<'_, keeless_lesswire::Result<bool>> {
        Box::pin(async { Ok(false) })
    }
}
