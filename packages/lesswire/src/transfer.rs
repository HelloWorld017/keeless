use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use getrandom::getrandom;
use zeroize::Zeroizing;

use crate::Clock;

pub const TRANSFER_MAGIC: u8 = b'!';
pub const TRANSFER_VERSION: u8 = 1;
pub const MAX_TRANSFER_SIZE: usize = 192 * 1024 * 1024;
pub const MAX_TRANSFER_BUFFER_SIZE: usize = MAX_TRANSFER_SIZE;
pub const TRANSFER_TTL_MS: u64 = 60_000;
pub const MAX_TRANSFER_CHUNK_SIZE: usize = 760 * 1024;

const HEADER_SIZE: usize = 3;
const ID_SIZE: usize = 16;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    BeginUpload = 1,
    BeginUploadResponse = 2,
    UploadChunk = 3,
    UploadChunkResponse = 4,
    BeginDownload = 5,
    BeginDownloadResponse = 6,
    DownloadChunk = 7,
    DownloadChunkResponse = 8,
    Abort = 9,
    Finish = 10,
}

impl TryFrom<u8> for Kind {
    type Error = ();

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::BeginUpload),
            2 => Ok(Self::BeginUploadResponse),
            3 => Ok(Self::UploadChunk),
            4 => Ok(Self::UploadChunkResponse),
            5 => Ok(Self::BeginDownload),
            6 => Ok(Self::BeginDownloadResponse),
            7 => Ok(Self::DownloadChunk),
            8 => Ok(Self::DownloadChunkResponse),
            9 => Ok(Self::Abort),
            10 => Ok(Self::Finish),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TransferId([u8; ID_SIZE]);

impl TransferId {
    fn generate() -> Result<Self> {
        let mut bytes = [0; ID_SIZE];
        getrandom(&mut bytes).map_err(|_| TransferError::Unavailable)?;
        Ok(Self(bytes))
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Some(Self(bytes.try_into().ok()?))
    }

    pub fn as_bytes(&self) -> &[u8; ID_SIZE] {
        &self.0
    }

    pub fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    pub fn parse(value: &str) -> Option<Self> {
        let bytes = URL_SAFE_NO_PAD.decode(value).ok()?;
        (URL_SAFE_NO_PAD.encode(&bytes) == value).then_some(())?;
        Self::from_bytes(&bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferOwner(String);

impl TransferOwner {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("transfer is unavailable")]
    Unavailable,
    #[error("transfer size is invalid")]
    InvalidSize,
    #[error("transfer capacity is exhausted")]
    CapacityExceeded,
    #[error("transfer does not exist")]
    NotFound,
    #[error("transfer belongs to another client")]
    OwnerMismatch,
    #[error("transfer is incomplete")]
    Incomplete,
    #[error("transfer offset is invalid")]
    InvalidOffset,
    #[error("transfer packet is invalid")]
    InvalidPacket,
}

type Result<T> = std::result::Result<T, TransferError>;

enum Transfer {
    Upload {
        bytes: Zeroizing<Vec<u8>>,
        next_offset: usize,
        finished: bool,
    },
    Download {
        bytes: Zeroizing<Vec<u8>>,
        started: bool,
    },
}

struct TransferEntry {
    owner: TransferOwner,
    last_activity_ms: u64,
    transfer: Transfer,
}

struct State {
    transfers: HashMap<TransferId, TransferEntry>,
    reserved_bytes: usize,
}

/// Bounded, short-lived binary transfers layered inside authenticated Lesswire payloads.
#[derive(Clone)]
pub struct TransferRegistry {
    clock: Arc<dyn Clock>,
    state: Arc<Mutex<State>>,
}

impl TransferRegistry {
    pub(crate) fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            state: Arc::new(Mutex::new(State {
                transfers: HashMap::new(),
                reserved_bytes: 0,
            })),
        }
    }

    pub fn publish_download(
        &self,
        owner: TransferOwner,
        bytes: Zeroizing<Vec<u8>>,
    ) -> Result<TransferId> {
        if bytes.is_empty() || bytes.len() > MAX_TRANSFER_SIZE {
            return Err(TransferError::InvalidSize);
        }
        let now = self.clock.monotonic_millis();
        let mut state = self.state.lock().map_err(|_| TransferError::Unavailable)?;
        purge_expired(&mut state, now);
        let id = fresh_id(&state)?;
        reserve(&mut state, bytes.len())?;
        state.transfers.insert(
            id.clone(),
            TransferEntry {
                owner,
                last_activity_ms: now,
                transfer: Transfer::Download {
                    bytes,
                    started: false,
                },
            },
        );
        Ok(id)
    }

    /// Moves a completed upload out of Lesswire so its consumer owns the only buffer.
    pub fn consume_upload(
        &self,
        owner: &TransferOwner,
        id: &TransferId,
    ) -> Result<Zeroizing<Vec<u8>>> {
        let now = self.clock.monotonic_millis();
        let mut state = self.state.lock().map_err(|_| TransferError::Unavailable)?;
        purge_expired(&mut state, now);
        let entry = state.transfers.remove(id).ok_or(TransferError::NotFound)?;
        let TransferEntry {
            owner: entry_owner,
            last_activity_ms,
            transfer,
        } = entry;
        if &entry_owner != owner {
            state.transfers.insert(
                id.clone(),
                TransferEntry {
                    owner: entry_owner,
                    last_activity_ms,
                    transfer,
                },
            );
            return Err(TransferError::OwnerMismatch);
        }
        match transfer {
            Transfer::Upload {
                bytes,
                next_offset,
                finished,
            } if finished && next_offset == bytes.len() => {
                state.reserved_bytes = state.reserved_bytes.saturating_sub(bytes.len());
                Ok(bytes)
            }
            transfer => {
                state.transfers.insert(
                    id.clone(),
                    TransferEntry {
                        owner: entry_owner,
                        last_activity_ms,
                        transfer,
                    },
                );
                Err(TransferError::Incomplete)
            }
        }
    }

    pub fn clear(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.transfers.clear();
            state.reserved_bytes = 0;
        }
    }

    pub fn purge_expired(&self) {
        if let Ok(mut state) = self.state.lock() {
            purge_expired(&mut state, self.clock.monotonic_millis());
        }
    }

    pub fn handle_packet(&self, owner: TransferOwner, payload: &[u8]) -> Result<Vec<u8>> {
        let packet = Packet::decode(payload)?;
        let now = self.clock.monotonic_millis();
        let mut state = self.state.lock().map_err(|_| TransferError::Unavailable)?;
        purge_expired(&mut state, now);
        match packet {
            Packet::BeginUpload { size } => {
                if size == 0 || size > MAX_TRANSFER_SIZE {
                    return Err(TransferError::InvalidSize);
                }
                let id = fresh_id(&state)?;
                reserve(&mut state, size)?;
                state.transfers.insert(
                    id.clone(),
                    TransferEntry {
                        owner,
                        last_activity_ms: now,
                        transfer: Transfer::Upload {
                            // Keep len equal to allocation size so Zeroizing clears all bytes on drop.
                            bytes: Zeroizing::new(vec![0; size]),
                            next_offset: 0,
                            finished: false,
                        },
                    },
                );
                Ok(Packet::BeginUploadResponse { id }.encode())
            }
            Packet::UploadChunk { id, offset, bytes } => {
                if bytes.len() > MAX_TRANSFER_CHUNK_SIZE {
                    return Err(TransferError::InvalidSize);
                }
                let entry = owned_entry(&mut state, &id, &owner)?;
                let Transfer::Upload {
                    bytes: buffer,
                    next_offset,
                    finished,
                } = &mut entry.transfer
                else {
                    return Err(TransferError::InvalidPacket);
                };
                if *finished || offset != *next_offset || bytes.len() > buffer.len() - *next_offset
                {
                    return Err(TransferError::InvalidOffset);
                }
                buffer[*next_offset..*next_offset + bytes.len()].copy_from_slice(bytes);
                *next_offset += bytes.len();
                entry.last_activity_ms = now;
                Ok(Packet::UploadChunkResponse {
                    id,
                    next_offset: *next_offset,
                }
                .encode())
            }
            Packet::BeginDownload { id } => {
                let entry = owned_entry(&mut state, &id, &owner)?;
                let Transfer::Download { bytes, started } = &mut entry.transfer else {
                    return Err(TransferError::InvalidPacket);
                };
                *started = true;
                entry.last_activity_ms = now;
                Ok(Packet::BeginDownloadResponse {
                    id,
                    size: bytes.len(),
                }
                .encode())
            }
            Packet::DownloadChunk { id, offset } => {
                let entry = owned_entry(&mut state, &id, &owner)?;
                let Transfer::Download { bytes, started } = &mut entry.transfer else {
                    return Err(TransferError::InvalidPacket);
                };
                if !*started || offset > bytes.len() {
                    return Err(TransferError::InvalidOffset);
                }
                let end = offset
                    .saturating_add(MAX_TRANSFER_CHUNK_SIZE)
                    .min(bytes.len());
                entry.last_activity_ms = now;
                Ok(Packet::DownloadChunkResponse {
                    id,
                    offset,
                    done: end == bytes.len(),
                    bytes: &bytes[offset..end],
                }
                .encode())
            }
            Packet::Abort { id } => {
                remove_owned(&mut state, &id, &owner)?;
                Ok(Packet::Abort { id }.encode())
            }
            Packet::Finish { id } => {
                let entry = owned_entry(&mut state, &id, &owner)?;
                let remove_download = match &mut entry.transfer {
                    Transfer::Upload {
                        bytes,
                        next_offset,
                        finished,
                    } => {
                        if *next_offset != bytes.len() {
                            return Err(TransferError::Incomplete);
                        }
                        *finished = true;
                        entry.last_activity_ms = now;
                        false
                    }
                    Transfer::Download { .. } => true,
                };
                if remove_download {
                    remove_owned(&mut state, &id, &owner)?;
                }
                Ok(Packet::Finish { id }.encode())
            }
            _ => Err(TransferError::InvalidPacket),
        }
    }
}

fn reserve(state: &mut State, size: usize) -> Result<()> {
    if size > MAX_TRANSFER_SIZE
        || state.reserved_bytes.saturating_add(size) > MAX_TRANSFER_BUFFER_SIZE
    {
        return Err(TransferError::CapacityExceeded);
    }
    state.reserved_bytes += size;
    Ok(())
}

fn fresh_id(state: &State) -> Result<TransferId> {
    loop {
        let id = TransferId::generate()?;
        if !state.transfers.contains_key(&id) {
            return Ok(id);
        }
    }
}

fn owned_entry<'a>(
    state: &'a mut State,
    id: &TransferId,
    owner: &TransferOwner,
) -> Result<&'a mut TransferEntry> {
    let entry = state.transfers.get_mut(id).ok_or(TransferError::NotFound)?;
    if &entry.owner != owner {
        return Err(TransferError::OwnerMismatch);
    }
    Ok(entry)
}

fn remove_owned(state: &mut State, id: &TransferId, owner: &TransferOwner) -> Result<()> {
    let entry = state.transfers.remove(id).ok_or(TransferError::NotFound)?;
    if &entry.owner != owner {
        state.transfers.insert(id.clone(), entry);
        return Err(TransferError::OwnerMismatch);
    }
    let size = match entry.transfer {
        Transfer::Upload { bytes, .. } | Transfer::Download { bytes, .. } => bytes.len(),
    };
    state.reserved_bytes = state.reserved_bytes.saturating_sub(size);
    Ok(())
}

fn purge_expired(state: &mut State, now: u64) {
    state.transfers.retain(|_, entry| {
        let expired = now.saturating_sub(entry.last_activity_ms) >= TRANSFER_TTL_MS;
        if expired {
            let size = match &entry.transfer {
                Transfer::Upload { bytes, .. } | Transfer::Download { bytes, .. } => bytes.len(),
            };
            state.reserved_bytes = state.reserved_bytes.saturating_sub(size);
        }
        !expired
    });
}

pub(crate) enum Packet<'a> {
    BeginUpload {
        size: usize,
    },
    BeginUploadResponse {
        id: TransferId,
    },
    UploadChunk {
        id: TransferId,
        offset: usize,
        bytes: &'a [u8],
    },
    UploadChunkResponse {
        id: TransferId,
        next_offset: usize,
    },
    BeginDownload {
        id: TransferId,
    },
    BeginDownloadResponse {
        id: TransferId,
        size: usize,
    },
    DownloadChunk {
        id: TransferId,
        offset: usize,
    },
    DownloadChunkResponse {
        id: TransferId,
        offset: usize,
        done: bool,
        bytes: &'a [u8],
    },
    Abort {
        id: TransferId,
    },
    Finish {
        id: TransferId,
    },
}

impl Packet<'_> {
    fn decode(payload: &[u8]) -> Result<Packet<'_>> {
        if payload.len() < HEADER_SIZE
            || payload[0] != TRANSFER_MAGIC
            || payload[1] != TRANSFER_VERSION
        {
            return Err(TransferError::InvalidPacket);
        }
        let kind = Kind::try_from(payload[2]).map_err(|_| TransferError::InvalidPacket)?;
        let body = &payload[HEADER_SIZE..];
        match kind {
            Kind::BeginUpload => Ok(Packet::BeginUpload {
                size: read_usize(body)?,
            }),
            Kind::UploadChunk => {
                let (id, body) = split_id(body)?;
                let (offset, bytes) = split_usize(body)?;
                Ok(Packet::UploadChunk { id, offset, bytes })
            }
            Kind::BeginDownload => Ok(Packet::BeginDownload { id: read_id(body)? }),
            Kind::DownloadChunk => {
                let (id, body) = split_id(body)?;
                Ok(Packet::DownloadChunk {
                    id,
                    offset: read_usize(body)?,
                })
            }
            Kind::Abort => Ok(Packet::Abort { id: read_id(body)? }),
            Kind::Finish => Ok(Packet::Finish { id: read_id(body)? }),
            _ => Err(TransferError::InvalidPacket),
        }
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![TRANSFER_MAGIC, TRANSFER_VERSION, self.kind() as u8];
        match self {
            Self::BeginUpload { size } => put_usize(&mut bytes, *size),
            Self::BeginUploadResponse { id }
            | Self::BeginDownload { id }
            | Self::Abort { id }
            | Self::Finish { id } => bytes.extend_from_slice(id.as_bytes()),
            Self::UploadChunk {
                id,
                offset,
                bytes: chunk,
            } => {
                bytes.extend_from_slice(id.as_bytes());
                put_usize(&mut bytes, *offset);
                bytes.extend_from_slice(chunk);
            }
            Self::UploadChunkResponse { id, next_offset } => {
                bytes.extend_from_slice(id.as_bytes());
                put_usize(&mut bytes, *next_offset);
            }
            Self::BeginDownloadResponse { id, size } => {
                bytes.extend_from_slice(id.as_bytes());
                put_usize(&mut bytes, *size);
            }
            Self::DownloadChunk { id, offset } => {
                bytes.extend_from_slice(id.as_bytes());
                put_usize(&mut bytes, *offset);
            }
            Self::DownloadChunkResponse {
                id,
                offset,
                done,
                bytes: chunk,
            } => {
                bytes.extend_from_slice(id.as_bytes());
                put_usize(&mut bytes, *offset);
                bytes.push(u8::from(*done));
                bytes.extend_from_slice(chunk);
            }
        }
        bytes
    }

    fn kind(&self) -> Kind {
        match self {
            Self::BeginUpload { .. } => Kind::BeginUpload,
            Self::BeginUploadResponse { .. } => Kind::BeginUploadResponse,
            Self::UploadChunk { .. } => Kind::UploadChunk,
            Self::UploadChunkResponse { .. } => Kind::UploadChunkResponse,
            Self::BeginDownload { .. } => Kind::BeginDownload,
            Self::BeginDownloadResponse { .. } => Kind::BeginDownloadResponse,
            Self::DownloadChunk { .. } => Kind::DownloadChunk,
            Self::DownloadChunkResponse { .. } => Kind::DownloadChunkResponse,
            Self::Abort { .. } => Kind::Abort,
            Self::Finish { .. } => Kind::Finish,
        }
    }
}

fn read_id(bytes: &[u8]) -> Result<TransferId> {
    if bytes.len() != ID_SIZE {
        return Err(TransferError::InvalidPacket);
    }
    TransferId::from_bytes(bytes).ok_or(TransferError::InvalidPacket)
}

fn split_id(bytes: &[u8]) -> Result<(TransferId, &[u8])> {
    let (id, rest) = bytes
        .split_at_checked(ID_SIZE)
        .ok_or(TransferError::InvalidPacket)?;
    Ok((
        TransferId::from_bytes(id).ok_or(TransferError::InvalidPacket)?,
        rest,
    ))
}

fn read_usize(bytes: &[u8]) -> Result<usize> {
    if bytes.len() != 8 {
        return Err(TransferError::InvalidPacket);
    }
    usize::try_from(u64::from_le_bytes(
        bytes.try_into().expect("length checked"),
    ))
    .map_err(|_| TransferError::InvalidSize)
}

fn split_usize(bytes: &[u8]) -> Result<(usize, &[u8])> {
    let (number, rest) = bytes
        .split_at_checked(8)
        .ok_or(TransferError::InvalidPacket)?;
    let value = usize::try_from(u64::from_le_bytes(
        number.try_into().expect("length checked"),
    ))
    .map_err(|_| TransferError::InvalidSize)?;
    Ok((value, rest))
}

fn put_usize(bytes: &mut Vec<u8>, value: usize) {
    bytes.extend_from_slice(&(value as u64).to_le_bytes());
}
