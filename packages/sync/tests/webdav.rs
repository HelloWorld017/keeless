use keeless_sync::{
    ByteRange, Revision, StorageErrorKind, StorageProvider, WebDavAuth, WebDavProvider,
    WriteCondition, WriteOutcome,
};
use wiremock::matchers::{header, header_regex, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn read_uses_basic_auth_and_slices_a_server_ignored_range() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/dav/vault.kdbx"))
        .and(header("authorization", "Basic dXNlcjpwYXNz"))
        .and(header("range", "bytes=2-4"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("ETag", "\"revision-1\"")
                .set_body_bytes(b"012345"),
        )
        .mount(&server)
        .await;

    let provider = WebDavProvider::new(
        format!("{}/dav", server.uri()),
        Some(WebDavAuth::basic("user", "pass")),
    )
    .unwrap();
    let file = provider
        .read("vault.kdbx", Some(ByteRange::new(2, 4).unwrap()))
        .await
        .unwrap();

    assert_eq!(file.bytes, b"234");
    assert_eq!(
        file.metadata.revision,
        Some(Revision::StrongEtag("\"revision-1\"".to_string()))
    );
}

#[tokio::test]
async fn write_maps_etag_conditions_and_precondition_failures() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/dav/vault.kdbx"))
        .and(header("if-match", "\"old\""))
        .respond_with(ResponseTemplate::new(204).insert_header("ETag", "\"new\""))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/dav/new.kdbx"))
        .and(header("if-none-match", "*"))
        .respond_with(ResponseTemplate::new(412))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/dav/missing/new.kdbx"))
        .and(header("if-none-match", "*"))
        .respond_with(ResponseTemplate::new(409))
        .expect(1)
        .mount(&server)
        .await;

    let provider = WebDavProvider::new(format!("{}/dav", server.uri()), None).unwrap();
    let updated = provider
        .write(
            "vault.kdbx",
            b"data".to_vec(),
            WriteCondition::MustMatch(Revision::StrongEtag("\"old\"".to_string())),
        )
        .await
        .unwrap();
    assert_eq!(
        updated,
        WriteOutcome::Applied {
            revision: Some(Revision::StrongEtag("\"new\"".to_string()))
        }
    );

    let created = provider
        .write("new.kdbx", b"data".to_vec(), WriteCondition::MustNotExist)
        .await
        .unwrap();
    assert_eq!(created, WriteOutcome::Conflict);

    let missing_parent = provider
        .write(
            "missing/new.kdbx",
            b"data".to_vec(),
            WriteCondition::MustNotExist,
        )
        .await
        .unwrap_err();
    assert_eq!(missing_parent.kind(), StorageErrorKind::Conflict);
}

#[tokio::test]
async fn weak_etag_falls_back_to_last_modified() {
    let server = MockServer::start().await;
    let modified = "Wed, 21 Oct 2015 07:28:00 GMT";
    Mock::given(method("GET"))
        .and(path("/dav/vault.kdbx"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("ETag", "W/\"weak\"")
                .insert_header("Last-Modified", modified)
                .set_body_bytes(b"old"),
        )
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/dav/vault.kdbx"))
        .and(header_regex(
            "if-unmodified-since",
            "^Wed, 21 Oct 2015 07:28:00 GMT$",
        ))
        .respond_with(
            ResponseTemplate::new(204)
                .insert_header("ETag", "W/\"weak-2\"")
                .insert_header("Last-Modified", "Wed, 21 Oct 2015 07:29:00 GMT"),
        )
        .mount(&server)
        .await;

    let provider = WebDavProvider::new(format!("{}/dav", server.uri()), None).unwrap();
    let remote = provider.read("vault.kdbx", None).await.unwrap();
    assert_eq!(
        remote.metadata.revision,
        Some(Revision::LastModified(modified.to_string()))
    );
    let outcome = provider
        .write(
            "vault.kdbx",
            b"new".to_vec(),
            WriteCondition::MustMatch(remote.metadata.revision.unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(
        outcome,
        WriteOutcome::Applied {
            revision: Some(Revision::LastModified(
                "Wed, 21 Oct 2015 07:29:00 GMT".to_string()
            ))
        }
    );
}

#[test]
fn webdav_debug_output_does_not_expose_passwords() {
    let provider = WebDavProvider::new(
        "https://example.com/dav?access_token=query-secret#fragment-secret",
        Some(WebDavAuth::basic("user", "secret-password")),
    )
    .unwrap();
    let debug = format!("{provider:?}");
    assert!(!debug.contains("secret-password"));
    assert!(!debug.contains("query-secret"));
    assert!(!debug.contains("fragment-secret"));
    assert!(debug.contains("[REDACTED]"));
    assert!(debug.contains("REDACTED"));
}

#[tokio::test]
async fn network_errors_do_not_expose_query_credentials() {
    let provider =
        WebDavProvider::new("http://127.0.0.1:0/dav?access_token=query-secret", None).unwrap();
    let error = provider.read("vault.kdbx", None).await.unwrap_err();
    let displayed = error.to_string();
    assert!(!displayed.contains("query-secret"));
    assert!(!displayed.contains("access_token"));
}
