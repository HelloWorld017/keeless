//! Length-bounded, current-user local IPC for the desktop daemon.

use std::io;

#[cfg(unix)]
use std::path::PathBuf;
#[cfg(windows)]
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const NAMESPACE: &str = "dev.nenw.keeless";
pub const MAX_MESSAGE_SIZE: usize = 2 * 1024 * 1024;
const PROTOCOL_VERSION: u8 = 1;

#[derive(Debug, Deserialize, Serialize)]
pub enum Request {
    Ping,
    HandleFrame(Vec<u8>),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum Response {
    Pong,
    Frame(Option<Vec<u8>>),
    Error(String),
}

#[derive(Debug, Deserialize, Serialize)]
struct Envelope<T> {
    version: u8,
    payload: T,
}

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("daemon IPC I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("daemon IPC message exceeds {MAX_MESSAGE_SIZE} bytes")]
    MessageTooLarge,
    #[error("invalid daemon IPC message: {0}")]
    Protocol(String),
    #[error("daemon rejected request: {0}")]
    Remote(String),
    #[error("another daemon is already running")]
    AlreadyRunning,
}

pub type Result<T> = std::result::Result<T, IpcError>;

pub struct Client;

impl Client {
    /// Open a connection without sending anything yet.
    ///
    /// Callers whose payload carries a timestamp should connect first and build
    /// the payload immediately before sending, so a slow connect cannot make an
    /// otherwise-valid message arrive outside its freshness window.
    pub async fn connect() -> Result<Connection> {
        Connection::connect().await
    }

    pub async fn request(request: Request) -> Result<Response> {
        let mut connection = Connection::connect().await?;
        connection.send_request(&request).await?;
        connection.receive_response().await
    }

    pub async fn ping() -> Result<()> {
        match Self::request(Request::Ping).await? {
            Response::Pong => Ok(()),
            _ => Err(IpcError::Protocol("unexpected ping response".into())),
        }
    }

    pub async fn handle_frame(frame: Vec<u8>) -> Result<Option<Vec<u8>>> {
        match Self::request(Request::HandleFrame(frame)).await? {
            Response::Frame(frame) => Ok(frame),
            _ => Err(IpcError::Protocol("unexpected frame response".into())),
        }
    }
}

#[cfg(unix)]
pub struct Connection {
    stream: PlatformStream,
}

#[cfg(unix)]
impl Connection {
    async fn connect() -> Result<Self> {
        connect_platform().await.map(|stream| Self { stream })
    }

    pub async fn send(&mut self, response: &Response) -> Result<()> {
        write_message(&mut self.stream, response).await
    }

    pub async fn receive(&mut self) -> Result<Request> {
        read_message(&mut self.stream).await
    }

    pub async fn send_request(&mut self, request: &Request) -> Result<()> {
        write_message(&mut self.stream, request).await
    }

    pub async fn receive_response(&mut self) -> Result<Response> {
        match read_message(&mut self.stream).await? {
            Response::Error(message) => Err(IpcError::Remote(message)),
            response => Ok(response),
        }
    }
}

#[cfg(windows)]
pub struct Connection {
    stream: WindowsStream,
}

#[cfg(windows)]
enum WindowsStream {
    Client(tokio::net::windows::named_pipe::NamedPipeClient),
    Server(tokio::net::windows::named_pipe::NamedPipeServer),
}

#[cfg(windows)]
impl Connection {
    async fn connect() -> Result<Self> {
        connect_platform().await.map(|stream| Self {
            stream: WindowsStream::Client(stream),
        })
    }

    pub async fn send(&mut self, response: &Response) -> Result<()> {
        match &mut self.stream {
            WindowsStream::Client(stream) => write_message(stream, response).await,
            WindowsStream::Server(stream) => write_message(stream, response).await,
        }
    }

    pub async fn receive(&mut self) -> Result<Request> {
        match &mut self.stream {
            WindowsStream::Client(stream) => read_message(stream).await,
            WindowsStream::Server(stream) => read_message(stream).await,
        }
    }

    pub async fn send_request(&mut self, request: &Request) -> Result<()> {
        match &mut self.stream {
            WindowsStream::Client(stream) => write_message(stream, request).await,
            WindowsStream::Server(stream) => write_message(stream, request).await,
        }
    }

    pub async fn receive_response(&mut self) -> Result<Response> {
        let response = match &mut self.stream {
            WindowsStream::Client(stream) => read_message(stream).await?,
            WindowsStream::Server(stream) => read_message(stream).await?,
        };
        match response {
            Response::Error(message) => Err(IpcError::Remote(message)),
            response => Ok(response),
        }
    }
}

pub struct ServerListener {
    inner: PlatformListener,
}

impl ServerListener {
    pub fn bind() -> Result<Self> {
        bind_platform().map(|inner| Self { inner })
    }

    pub async fn accept(&mut self) -> Result<Connection> {
        let stream = accept_platform(&mut self.inner).await?;
        #[cfg(unix)]
        return Ok(Connection { stream });
        #[cfg(windows)]
        return Ok(Connection {
            stream: WindowsStream::Server(stream),
        });
    }
}

async fn write_message<W, T>(writer: &mut W, payload: &T) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let bytes = postcard::to_stdvec(&Envelope {
        version: PROTOCOL_VERSION,
        payload,
    })
    .map_err(|error| IpcError::Protocol(error.to_string()))?;
    if bytes.len() > MAX_MESSAGE_SIZE {
        return Err(IpcError::MessageTooLarge);
    }
    writer.write_u32_le(bytes.len() as u32).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

async fn read_message<R, T>(reader: &mut R) -> Result<T>
where
    R: AsyncRead + Unpin,
    T: for<'de> Deserialize<'de>,
{
    let length = reader.read_u32_le().await? as usize;
    if length > MAX_MESSAGE_SIZE {
        return Err(IpcError::MessageTooLarge);
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).await?;
    let envelope: Envelope<T> =
        postcard::from_bytes(&bytes).map_err(|error| IpcError::Protocol(error.to_string()))?;
    if envelope.version != PROTOCOL_VERSION {
        return Err(IpcError::Protocol("unsupported protocol version".into()));
    }
    Ok(envelope.payload)
}

#[cfg(unix)]
type PlatformStream = tokio::net::UnixStream;
#[cfg(unix)]
type PlatformListener = tokio::net::UnixListener;

#[cfg(unix)]
pub fn endpoint_path() -> PathBuf {
    let root = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("{NAMESPACE}-{}", unsafe { libc::geteuid() }))
        });
    root.join(NAMESPACE).join("daemon.sock")
}

#[cfg(unix)]
fn bind_platform() -> Result<PlatformListener> {
    use std::os::unix::fs::PermissionsExt;

    let path = endpoint_path();
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid IPC path"))?;
    std::fs::create_dir_all(parent)?;
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    let listener = tokio::net::UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

#[cfg(unix)]
async fn connect_platform() -> Result<PlatformStream> {
    Ok(tokio::net::UnixStream::connect(endpoint_path()).await?)
}

#[cfg(unix)]
async fn accept_platform(listener: &mut PlatformListener) -> Result<PlatformStream> {
    loop {
        let (stream, _) = listener.accept().await?;
        let peer_uid = stream.peer_cred()?.uid();
        if peer_uid == unsafe { libc::geteuid() } {
            return Ok(stream);
        }
    }
}

#[cfg(windows)]
struct PlatformListener {
    pending: Option<tokio::net::windows::named_pipe::NamedPipeServer>,
}

#[cfg(windows)]
fn pipe_name() -> &'static str {
    r"\\.\pipe\dev.nenw.keeless.daemon"
}

#[cfg(windows)]
fn new_pipe(first: bool) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true);
    options.create(pipe_name())
}

#[cfg(windows)]
fn bind_platform() -> Result<PlatformListener> {
    let pending = new_pipe(true).map_err(|error| {
        if error.kind() == io::ErrorKind::PermissionDenied {
            IpcError::AlreadyRunning
        } else {
            IpcError::Io(error)
        }
    })?;
    Ok(PlatformListener {
        pending: Some(pending),
    })
}

#[cfg(windows)]
async fn connect_platform() -> Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    use tokio::net::windows::named_pipe::ClientOptions;

    for _ in 0..50 {
        match ClientOptions::new().open(pipe_name()) {
            Ok(client) => return Ok(client),
            Err(error)
                if error.raw_os_error()
                    == Some(windows_sys::Win32::Foundation::ERROR_PIPE_BUSY as i32) =>
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "named pipe remained busy").into())
}

#[cfg(windows)]
async fn accept_platform(
    listener: &mut PlatformListener,
) -> Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    let server = listener
        .pending
        .as_ref()
        .ok_or_else(|| IpcError::Protocol("named-pipe listener is unavailable".into()))?;
    server.connect().await?;
    let server = listener
        .pending
        .take()
        .ok_or_else(|| IpcError::Protocol("named-pipe listener is unavailable".into()))?;
    listener.pending = Some(new_pipe(false)?);
    Ok(server)
}

#[cfg(not(any(unix, windows)))]
compile_error!("keeless_host_client supports Unix and Windows only");

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_oversized_length_before_allocating() {
        let mut input = Vec::new();
        input.extend_from_slice(&((MAX_MESSAGE_SIZE as u32) + 1).to_le_bytes());
        let error = read_message::<_, Request>(&mut input.as_slice())
            .await
            .unwrap_err();
        assert!(matches!(error, IpcError::MessageTooLarge));
    }

    #[tokio::test]
    async fn protocol_round_trip_preserves_raw_bytes() {
        let request = Request::HandleFrame(vec![0, 1, 2, 255]);
        let mut bytes = Vec::new();
        write_message(&mut bytes, &request).await.unwrap();
        let decoded: Request = read_message(&mut bytes.as_slice()).await.unwrap();
        assert!(matches!(decoded, Request::HandleFrame(value) if value == vec![0, 1, 2, 255]));
    }
}
