use futures_util::StreamExt;
use reqwest::header::{
    CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_MATCH, IF_NONE_MATCH,
    IF_UNMODIFIED_SINCE, LAST_MODIFIED, RANGE,
};
use reqwest::{Client, Method, StatusCode};
use url::Url;
use zeroize::Zeroizing;

use crate::{
    ByteRange, FileMetadata, RemoteFile, Revision, StorageError, StorageErrorKind, StorageFuture,
    StorageProvider, WriteCondition, WriteOutcome,
};

const DEFAULT_MAX_FILE_SIZE: usize = 512 * 1024 * 1024;

#[derive(Clone)]
pub struct WebDavAuth {
    username: String,
    password: Zeroizing<String>,
}

impl WebDavAuth {
    pub fn basic(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: Zeroizing::new(password.into()),
        }
    }

    pub fn username(&self) -> &str {
        &self.username
    }
}

impl std::fmt::Debug for WebDavAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebDavAuth")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone)]
pub struct WebDavProvider {
    client: Client,
    base_url: Url,
    auth: Option<WebDavAuth>,
    max_file_size: usize,
}

impl std::fmt::Debug for WebDavProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut safe_url = self.base_url.clone();
        if safe_url.query().is_some() {
            safe_url.set_query(Some("REDACTED"));
        }
        if safe_url.fragment().is_some() {
            safe_url.set_fragment(Some("REDACTED"));
        }
        f.debug_struct("WebDavProvider")
            .field("base_url", &safe_url)
            .field("auth", &self.auth)
            .field("max_file_size", &self.max_file_size)
            .finish_non_exhaustive()
    }
}

impl WebDavProvider {
    pub fn new(base_url: impl AsRef<str>, auth: Option<WebDavAuth>) -> Result<Self, StorageError> {
        let mut base_url = Url::parse(base_url.as_ref()).map_err(|error| {
            StorageError::new(
                StorageErrorKind::InvalidInput,
                format!("invalid WebDAV URL: {error}"),
            )
        })?;
        if base_url.cannot_be_a_base() {
            return Err(StorageError::new(
                StorageErrorKind::InvalidInput,
                "WebDAV URL cannot be used as a base URL",
            ));
        }
        if !matches!(base_url.scheme(), "http" | "https") {
            return Err(StorageError::new(
                StorageErrorKind::InvalidInput,
                "WebDAV URL must use HTTP or HTTPS",
            ));
        }
        if !base_url.username().is_empty() || base_url.password().is_some() {
            return Err(StorageError::new(
                StorageErrorKind::InvalidInput,
                "WebDAV credentials must be provided through WebDavAuth",
            ));
        }
        if !base_url.path().ends_with('/') {
            let path = format!("{}/", base_url.path());
            base_url.set_path(&path);
        }

        Ok(Self {
            client: Client::new(),
            base_url,
            auth,
            max_file_size: DEFAULT_MAX_FILE_SIZE,
        })
    }

    pub fn with_max_file_size(mut self, max_file_size: usize) -> Self {
        self.max_file_size = max_file_size;
        self
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    fn url_for(&self, path: &str) -> Result<Url, StorageError> {
        let mut url = self.base_url.clone();
        {
            let mut segments = url.path_segments_mut().map_err(|_| {
                StorageError::new(
                    StorageErrorKind::InvalidInput,
                    "WebDAV URL does not support path segments",
                )
            })?;
            segments.pop_if_empty();
            for segment in normalized_segments(path)? {
                segments.push(segment);
            }
        }
        Ok(url)
    }

    fn request(&self, method: Method, url: Url) -> reqwest::RequestBuilder {
        let request = self.client.request(method, url);
        if let Some(auth) = &self.auth {
            request.basic_auth(auth.username(), Some(auth.password.as_str()))
        } else {
            request
        }
    }

    async fn read_impl(
        &self,
        path: &str,
        range: Option<ByteRange>,
    ) -> Result<RemoteFile, StorageError> {
        let url = self.url_for(path)?;
        let mut request = self.request(Method::GET, url);
        if let Some(range) = range {
            request = request.header(RANGE, format!("bytes={}-{}", range.start, range.end));
        }
        let response = request.send().await.map_err(network_error)?;
        if !response.status().is_success() {
            return Err(status_error("read", path, response.status()));
        }

        let status = response.status();
        let headers = response.headers().clone();
        if let Some(length) = parse_content_length(&headers) {
            if length > self.max_file_size as u64 && range.is_none() {
                return Err(StorageError::new(
                    StorageErrorKind::InvalidInput,
                    format!("WebDAV file exceeds {} bytes: {path}", self.max_file_size),
                ));
            }
        }

        let mut bytes = read_limited(response, self.max_file_size, "WebDAV file", path).await?;

        if let Some(range) = range {
            let expected = range.end.saturating_sub(range.start).saturating_add(1);
            if status == StatusCode::OK {
                let start = usize::try_from(range.start)
                    .unwrap_or(usize::MAX)
                    .min(bytes.len());
                let end = usize::try_from(range.end.saturating_add(1))
                    .unwrap_or(usize::MAX)
                    .min(bytes.len());
                bytes = bytes[start..end.max(start)].to_vec();
            } else if bytes.len() as u64 > expected {
                bytes.truncate(expected as usize);
            }
        }

        let last_modified = header_string(&headers, LAST_MODIFIED);
        Ok(RemoteFile {
            metadata: FileMetadata {
                size: bytes.len() as u64,
                revision: revision_from_headers(&headers),
                last_modified,
            },
            bytes,
        })
    }
}

impl StorageProvider for WebDavProvider {
    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<ByteRange>,
    ) -> StorageFuture<'a, Result<RemoteFile, StorageError>> {
        Box::pin(async move { self.read_impl(path, range).await })
    }

    fn stat<'a>(
        &'a self,
        path: &'a str,
    ) -> StorageFuture<'a, Result<Option<FileMetadata>, StorageError>> {
        Box::pin(async move {
            let response = self
                .request(Method::HEAD, self.url_for(path)?)
                .header(CACHE_CONTROL, "no-cache, no-store")
                .send()
                .await
                .map_err(network_error)?;
            if response.status() == StatusCode::NOT_FOUND {
                return Ok(None);
            }
            if !response.status().is_success() {
                return Err(status_error("stat", path, response.status()));
            }
            let headers = response.headers();
            Ok(Some(FileMetadata {
                size: parse_content_length(headers).unwrap_or(0),
                revision: revision_from_headers(headers),
                last_modified: header_string(headers, LAST_MODIFIED),
            }))
        })
    }

    fn write<'a>(
        &'a self,
        path: &'a str,
        bytes: Vec<u8>,
        condition: WriteCondition,
    ) -> StorageFuture<'a, Result<WriteOutcome, StorageError>> {
        Box::pin(async move {
            let bytes = bytes::Bytes::from(bytes);
            let mut request = self
                .request(Method::PUT, self.url_for(path)?)
                .header(CONTENT_TYPE, "application/octet-stream");
            request = match condition {
                WriteCondition::Unconditional => request,
                WriteCondition::MustNotExist => request.header(IF_NONE_MATCH, "*"),
                WriteCondition::MustMatch(Revision::StrongEtag(etag)) => {
                    request.header(IF_MATCH, etag)
                }
                WriteCondition::MustMatch(Revision::LastModified(value)) => {
                    request.header(IF_UNMODIFIED_SINCE, value)
                }
            };

            let response = request
                .body(bytes.clone())
                .send()
                .await
                .map_err(network_error)?;
            if response.status() == StatusCode::PRECONDITION_FAILED {
                return Ok(WriteOutcome::Conflict);
            }
            if !response.status().is_success() {
                return Err(status_error("write", path, response.status()));
            }

            if let Some(revision) = revision_from_headers(response.headers()) {
                return Ok(WriteOutcome::Applied {
                    revision: Some(revision),
                });
            }

            let remote = self.read_impl(path, None).await?;
            if remote.bytes.as_slice() != bytes.as_ref() {
                return Ok(WriteOutcome::Conflict);
            }
            Ok(WriteOutcome::Applied {
                revision: remote.metadata.revision,
            })
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> StorageFuture<'a, Result<(), StorageError>> {
        Box::pin(async move {
            let response = self
                .request(Method::DELETE, self.url_for(path)?)
                .send()
                .await
                .map_err(network_error)?;
            if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
                return Ok(());
            }
            Err(status_error("delete", path, response.status()))
        })
    }
}

fn normalized_segments(path: &str) -> Result<Vec<&str>, StorageError> {
    let segments: Vec<_> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments
        .iter()
        .any(|segment| *segment == "." || *segment == "..")
    {
        return Err(StorageError::new(
            StorageErrorKind::InvalidInput,
            "WebDAV path must not contain '.' or '..' segments",
        ));
    }
    Ok(segments)
}

async fn read_limited(
    response: reqwest::Response,
    limit: usize,
    kind: &str,
    path: &str,
) -> Result<Vec<u8>, StorageError> {
    if parse_content_length(response.headers()).is_some_and(|length| length > limit as u64) {
        return Err(StorageError::new(
            StorageErrorKind::InvalidInput,
            format!("{kind} exceeds {limit} bytes: {path}"),
        ));
    }

    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(network_error)?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(StorageError::new(
                StorageErrorKind::InvalidInput,
                format!("{kind} exceeds {limit} bytes: {path}"),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn revision_from_headers(headers: &reqwest::header::HeaderMap) -> Option<Revision> {
    if let Some(etag) = header_string(headers, ETAG) {
        if !etag.trim_start().to_ascii_lowercase().starts_with("w/") && !etag.trim().is_empty() {
            return Some(Revision::StrongEtag(etag));
        }
    }
    let last_modified = header_string(headers, LAST_MODIFIED)?;
    httpdate::parse_http_date(&last_modified).ok()?;
    Some(Revision::LastModified(last_modified))
}

fn header_string(
    headers: &reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn parse_content_length(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
}

fn network_error(error: reqwest::Error) -> StorageError {
    StorageError::new(StorageErrorKind::Network, error.without_url().to_string())
}

fn status_error(operation: &str, path: &str, status: StatusCode) -> StorageError {
    let kind = match status {
        StatusCode::NOT_FOUND => StorageErrorKind::NotFound,
        StatusCode::UNAUTHORIZED => StorageErrorKind::Authentication,
        StatusCode::FORBIDDEN => StorageErrorKind::PermissionDenied,
        StatusCode::CONFLICT | StatusCode::PRECONDITION_FAILED => StorageErrorKind::Conflict,
        status if status.is_server_error() => StorageErrorKind::Server,
        _ => StorageErrorKind::Other,
    };
    StorageError::new(
        kind,
        format!("WebDAV {operation} failed for {path}: HTTP {status}"),
    )
}
