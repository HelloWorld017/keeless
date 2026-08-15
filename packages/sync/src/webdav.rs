use futures_util::StreamExt;
use percent_encoding::percent_decode_str;
use reqwest::header::{
    CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_MATCH, IF_NONE_MATCH,
    IF_UNMODIFIED_SINCE, LAST_MODIFIED, RANGE,
};
use reqwest::{Client, Method, StatusCode};
use url::Url;

use crate::{
    ByteRange, FileMetadata, RemoteFile, Revision, StorageError, StorageErrorKind, StorageFuture,
    StorageProvider, WriteCondition, WriteOutcome,
};

const DEFAULT_MAX_FILE_SIZE: usize = 512 * 1024 * 1024;

#[derive(Clone)]
pub struct WebDavProvider {
    client: Client,
    max_file_size: usize,
}

impl std::fmt::Debug for WebDavProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebDavProvider")
            .field("max_file_size", &self.max_file_size)
            .finish_non_exhaustive()
    }
}

impl Default for WebDavProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl WebDavProvider {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            max_file_size: DEFAULT_MAX_FILE_SIZE,
        }
    }

    pub fn with_max_file_size(mut self, max_file_size: usize) -> Self {
        self.max_file_size = max_file_size;
        self
    }

    pub fn get_normalized_path(path: &str) -> Result<String, StorageError> {
        let mut url = parse_url(path)?;
        url.set_username("").map_err(|_| {
            StorageError::new(
                StorageErrorKind::InvalidInput,
                "invalid WebDAV URL username",
            )
        })?;
        url.set_password(None).map_err(|_| {
            StorageError::new(
                StorageErrorKind::InvalidInput,
                "invalid WebDAV URL password",
            )
        })?;
        Ok(url.into())
    }

    fn request_url(&self, path: &str) -> Result<(Url, Option<(String, String)>), StorageError> {
        let mut url = parse_url(path)?;
        let username = decode_userinfo(url.username())?;
        let password = url.password().map(decode_userinfo).transpose()?;
        let auth = match (username.is_empty(), password) {
            (true, None) => None,
            (false, Some(password)) => Some((username, password)),
            _ => {
                return Err(StorageError::new(
                    StorageErrorKind::InvalidInput,
                    "WebDAV URL must contain both username and password",
                ));
            }
        };
        url.set_username("").map_err(|_| {
            StorageError::new(
                StorageErrorKind::InvalidInput,
                "invalid WebDAV URL username",
            )
        })?;
        url.set_password(None).map_err(|_| {
            StorageError::new(
                StorageErrorKind::InvalidInput,
                "invalid WebDAV URL password",
            )
        })?;
        Ok((url, auth))
    }

    fn request(
        &self,
        method: Method,
        url: Url,
        auth: Option<(String, String)>,
    ) -> reqwest::RequestBuilder {
        let request = self.client.request(method, url);
        if let Some((username, password)) = auth {
            request.basic_auth(username, Some(password))
        } else {
            request
        }
    }

    async fn read_impl(
        &self,
        path: &str,
        range: Option<ByteRange>,
    ) -> Result<RemoteFile, StorageError> {
        let (url, auth) = self.request_url(path)?;
        let mut request = self.request(Method::GET, url, auth);
        if let Some(range) = range {
            request = request.header(RANGE, format!("bytes={}-{}", range.start, range.end));
        }
        let response = request.send().await.map_err(network_error)?;
        if !response.status().is_success() {
            return Err(status_error("read", response.status()));
        }

        let status = response.status();
        let headers = response.headers().clone();
        if let Some(length) = parse_content_length(&headers) {
            if length > self.max_file_size as u64 && range.is_none() {
                return Err(StorageError::new(
                    StorageErrorKind::InvalidInput,
                    format!("WebDAV file exceeds {} bytes", self.max_file_size),
                ));
            }
        }

        let mut bytes = read_limited(response, self.max_file_size, "WebDAV file").await?;

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
            let (url, auth) = self.request_url(path)?;
            let response = self
                .request(Method::HEAD, url, auth)
                .header(CACHE_CONTROL, "no-cache, no-store")
                .send()
                .await
                .map_err(network_error)?;
            if response.status() == StatusCode::NOT_FOUND {
                return Ok(None);
            }
            if !response.status().is_success() {
                return Err(status_error("stat", response.status()));
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
            let (url, auth) = self.request_url(path)?;
            let mut request = self
                .request(Method::PUT, url, auth)
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
                return Err(status_error("write", response.status()));
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
            let (url, auth) = self.request_url(path)?;
            let response = self
                .request(Method::DELETE, url, auth)
                .send()
                .await
                .map_err(network_error)?;
            if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
                return Ok(());
            }
            Err(status_error("delete", response.status()))
        })
    }
}

async fn read_limited(
    response: reqwest::Response,
    limit: usize,
    kind: &str,
) -> Result<Vec<u8>, StorageError> {
    if parse_content_length(response.headers()).is_some_and(|length| length > limit as u64) {
        return Err(StorageError::new(
            StorageErrorKind::InvalidInput,
            format!("{kind} exceeds {limit} bytes"),
        ));
    }

    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(network_error)?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(StorageError::new(
                StorageErrorKind::InvalidInput,
                format!("{kind} exceeds {limit} bytes"),
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

fn parse_url(path: &str) -> Result<Url, StorageError> {
    let url = Url::parse(path)
        .map_err(|_| StorageError::new(StorageErrorKind::InvalidInput, "invalid WebDAV URL"))?;
    if url.cannot_be_a_base() || !matches!(url.scheme(), "http" | "https") {
        return Err(StorageError::new(
            StorageErrorKind::InvalidInput,
            "WebDAV URL must use HTTP or HTTPS",
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(StorageError::new(
            StorageErrorKind::InvalidInput,
            "WebDAV URL must not contain a query or fragment",
        ));
    }
    Ok(url)
}

fn decode_userinfo(value: &str) -> Result<String, StorageError> {
    percent_decode_str(value)
        .decode_utf8()
        .map(|value| value.into_owned())
        .map_err(|_| StorageError::new(StorageErrorKind::InvalidInput, "invalid WebDAV credential"))
}

fn status_error(operation: &str, status: StatusCode) -> StorageError {
    let kind = match status {
        StatusCode::NOT_FOUND => StorageErrorKind::NotFound,
        StatusCode::UNAUTHORIZED => StorageErrorKind::Authentication,
        StatusCode::FORBIDDEN => StorageErrorKind::PermissionDenied,
        StatusCode::CONFLICT | StatusCode::PRECONDITION_FAILED => StorageErrorKind::Conflict,
        status if status.is_server_error() => StorageErrorKind::Server,
        _ => StorageErrorKind::Other,
    };
    StorageError::new(kind, format!("WebDAV {operation} failed: HTTP {status}"))
}
