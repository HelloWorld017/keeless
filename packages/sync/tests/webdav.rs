use keeless_sync::{
    ByteRange, Revision, StorageErrorKind, StorageProvider, WebDavProvider, WriteCondition,
    WriteOutcome,
};
use wiremock::matchers::{header, header_regex, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn read_uses_basic_auth_and_slices_a_server_ignored_range() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/dav/vault.kdbx"))
        .and(header("authorization", "Basic dXNlcjpwYTpzcw=="))
        .and(header("range", "bytes=2-4"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("ETag", "\"revision-1\"")
                .set_body_bytes(b"012345"),
        )
        .mount(&server)
        .await;

    let provider = WebDavProvider::new();
    let path = format!(
        "{}{}",
        server.uri().replacen("http://", "http://user:pa%3Ass@", 1),
        "/dav/vault.kdbx"
    );
    let file = provider
        .read(&path, Some(ByteRange::new(2, 4).unwrap()))
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

    let provider = WebDavProvider::new();
    let updated = provider
        .write(
            &format!("{}/dav/vault.kdbx", server.uri()),
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
        .write(
            &format!("{}/dav/new.kdbx", server.uri()),
            b"data".to_vec(),
            WriteCondition::MustNotExist,
        )
        .await
        .unwrap();
    assert_eq!(created, WriteOutcome::Conflict);

    let missing_parent = provider
        .write(
            &format!("{}/dav/missing/new.kdbx", server.uri()),
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

    let provider = WebDavProvider::new();
    let path = format!("{}/dav/vault.kdbx", server.uri());
    let remote = provider.read(&path, None).await.unwrap();
    assert_eq!(
        remote.metadata.revision,
        Some(Revision::LastModified(modified.to_string()))
    );
    let outcome = provider
        .write(
            &path,
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
fn webdav_normalized_path_omits_credentials() {
    let normalized = WebDavProvider::get_normalized_path(
        "https://user:secret-password@example.com/dav/vault.kdbx",
    )
    .unwrap();
    assert_eq!(normalized, "https://example.com/dav/vault.kdbx");
}

#[test]
fn webdav_debug_output_does_not_expose_paths() {
    let provider = WebDavProvider::new();
    let debug = format!("{provider:?}");
    assert!(!debug.contains("secret-password"));
    assert!(!debug.contains("example.com"));
}

#[tokio::test]
async fn network_errors_do_not_expose_query_credentials() {
    let provider = WebDavProvider::new();
    let error = provider
        .read("http://user:query-secret@127.0.0.1:0/dav/vault.kdbx", None)
        .await
        .unwrap_err();
    let displayed = error.to_string();
    assert!(!displayed.contains("query-secret"));
    assert!(!displayed.contains("user"));
}
