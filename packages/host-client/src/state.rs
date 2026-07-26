//! Persisted wire identity for a sidecar process.

use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use keeless_lesswire::{Identity, StateStore, WireFuture};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::fs;

pub const MAX_STATE_SIZE: usize = 64 * 1024;
const STATE_VERSION: u8 = 1;

/// An owner-only file replaced atomically on every write.
#[derive(Debug, Clone)]
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    /// A file in the shared Keeless data directory, alongside the desktop app's own state.
    pub fn project(file_name: &str) -> io::Result<Self> {
        let dirs = ProjectDirs::from("dev", "nenw", "keeless")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no user data directory"))?;
        Ok(Self::at(dirs.data_dir().join(file_name)))
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn directory(&self) -> io::Result<&Path> {
        self.path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "state path has no parent"))
    }

    /// Read the file, or `None` when it does not exist yet.
    ///
    /// Reads at most `limit` bytes and fails past it, so a corrupt or hostile file
    /// cannot force an unbounded allocation.
    pub async fn load(&self, limit: usize) -> io::Result<Option<Zeroizing<Vec<u8>>>> {
        let file = match tokio::fs::File::open(&self.path).await {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut bytes = Zeroizing::new(Vec::new());
        file.take((limit + 1) as u64)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "state file is too large",
            ));
        }
        Ok(Some(bytes))
    }

    /// Replace the file's contents, leaving the previous contents intact on failure.
    pub async fn save(&self, contents: &[u8]) -> io::Result<()> {
        let directory = self.directory()?;
        tokio::fs::create_dir_all(directory).await?;
        fs::set_directory_permissions(directory)?;

        let temporary = fs::temporary_path(&self.path)?;
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        fs::set_private_create_mode(&mut options);
        let mut file = options.open(&temporary).await?;
        let result = async {
            fs::set_file_permissions(&file).await?;
            file.write_all(contents).await?;
            file.sync_all().await?;
            drop(file);
            fs::replace_file(&temporary, &self.path)?;
            fs::sync_directory(directory)
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&temporary).await;
        }
        result
    }
}

impl StateStore for FileStore {
    fn load(&self) -> WireFuture<'_, keeless_lesswire::Result<Option<Vec<u8>>>> {
        Box::pin(async move {
            FileStore::load(self, MAX_STATE_SIZE)
                .await
                .map(|state| state.map(|bytes| bytes.to_vec()))
                .map_err(|error| keeless_lesswire::Error::Host(error.to_string()))
        })
    }

    fn save<'a>(&'a self, state: &'a [u8]) -> WireFuture<'a, keeless_lesswire::Result<()>> {
        Box::pin(async move {
            FileStore::save(self, state)
                .await
                .map_err(|error| keeless_lesswire::Error::Host(error.to_string()))
        })
    }
}

/// Holds the identity in hex, so it is wiped rather than left in the heap.
#[derive(Deserialize, Serialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
struct PersistedClientState {
    #[zeroize(skip)]
    version: u8,
    /// The client's own lesswire identity, base16 of its 64 secret bytes.
    identity: String,
    /// The host key this client pinned on first contact, absent until paired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[zeroize(skip)]
    trusted_server: Option<String>,
}

/// A sidecar's lesswire identity together with the host key it trusts.
///
/// The identity is generated on first use and reused afterwards, so the host's
/// approval survives restarts; the pinned host key makes a replaced host visible
/// instead of silently trusted.
pub struct ClientState {
    store: FileStore,
    identity: Zeroizing<[u8; 64]>,
    trusted_server: Option<String>,
}

impl ClientState {
    /// Load the state, generating and persisting a fresh identity when absent.
    pub async fn load(store: FileStore) -> Result<Self, StateError> {
        if let Some(bytes) = store.load(MAX_STATE_SIZE).await? {
            let persisted: PersistedClientState =
                serde_json::from_slice(&bytes).map_err(|_| StateError::Malformed)?;
            if persisted.version != STATE_VERSION {
                return Err(StateError::Malformed);
            }
            let decoded = Zeroizing::new(
                hex::decode(&persisted.identity).map_err(|_| StateError::Malformed)?,
            );
            let identity: [u8; 64] = decoded
                .as_slice()
                .try_into()
                .map_err(|_| StateError::Malformed)?;
            return Ok(Self {
                store,
                identity: Zeroizing::new(identity),
                trusted_server: persisted.trusted_server.clone(),
            });
        }

        let state = Self {
            store,
            identity: Identity::generate()?.to_bytes(),
            trusted_server: None,
        };
        state.persist().await?;
        Ok(state)
    }

    pub fn identity(&self) -> Result<Identity, StateError> {
        Ok(Identity::from_bytes(self.identity.as_slice())?)
    }

    pub fn trusted_server(&self) -> Option<&str> {
        self.trusted_server.as_deref()
    }

    /// Pin the host key learned from a handshake.
    pub async fn set_trusted_server(&mut self, bundle: String) -> Result<(), StateError> {
        let previous = self.trusted_server.replace(bundle);
        if let Err(error) = self.persist().await {
            self.trusted_server = previous;
            return Err(error);
        }
        Ok(())
    }

    /// Forget the pinned host key so the next handshake pairs again.
    pub async fn reset_pairing(&mut self) -> Result<(), StateError> {
        let previous = self.trusted_server.take();
        if let Err(error) = self.persist().await {
            self.trusted_server = previous;
            return Err(error);
        }
        Ok(())
    }

    async fn persist(&self) -> Result<(), StateError> {
        let persisted = PersistedClientState {
            version: STATE_VERSION,
            identity: hex::encode(self.identity.as_slice()),
            trusted_server: self.trusted_server.clone(),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&persisted)?);
        self.store.save(&bytes).await?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("client state I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("client state is malformed")]
    Malformed,
    #[error("client state serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error(transparent)]
    Wire(#[from] keeless_lesswire::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn generates_an_identity_once_and_reuses_it() {
        let directory = tempfile::tempdir().unwrap();
        let store = FileStore::at(directory.path().join("nested/vhid-state.json"));

        let mut state = ClientState::load(store.clone()).await.unwrap();
        let bundle = state.identity().unwrap().public_key_bundle();
        assert!(state.trusted_server().is_none());

        state.set_trusted_server("v1.server".into()).await.unwrap();

        let reloaded = ClientState::load(store.clone()).await.unwrap();
        assert_eq!(reloaded.identity().unwrap().public_key_bundle(), bundle);
        assert_eq!(reloaded.trusted_server(), Some("v1.server"));

        let mut reloaded = reloaded;
        reloaded.reset_pairing().await.unwrap();
        assert!(
            ClientState::load(store)
                .await
                .unwrap()
                .trusted_server()
                .is_none()
        );
    }

    #[tokio::test]
    async fn rejects_malformed_state_instead_of_replacing_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vhid-state.json");
        tokio::fs::write(&path, b"{\"version\":9,\"identity\":\"00\"}")
            .await
            .unwrap();
        let error = ClientState::load(FileStore::at(path.clone()))
            .await
            .err()
            .expect("a malformed state file should be rejected");
        assert!(matches!(error, StateError::Malformed));
        assert!(tokio::fs::try_exists(&path).await.unwrap());
    }

    #[tokio::test]
    async fn refuses_oversized_state_files() {
        let directory = tempfile::tempdir().unwrap();
        let store = FileStore::at(directory.path().join("state.json"));
        store.save(&[b'x'; 16]).await.unwrap();
        let error = store.load(8).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
