//! Persisted wire identity for a sidecar process.

use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use keeless_lesswire::{Identity, KeyScope, PublicKeyBundle, StateStore, WireFuture};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::fs;

pub const MAX_STATE_SIZE: usize = 64 * 1024;
const STATE_VERSION: u8 = 3;

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
        #[cfg(windows)]
        if self.path.exists() {
            // Pairing identities are credentials. Repair an ACL left by an older
            // version before opening an existing state file.
            fs::ensure_file_permissions(&self.path)?;
        }
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
            fs::set_file_permissions(&file, &temporary).await?;
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
    #[zeroize(skip)]
    scope: KeyScope,
    /// Stable endpoint IDs mapped to server bundles pinned after a handshake.
    #[serde(default)]
    #[zeroize(skip)]
    trusted_servers: std::collections::HashMap<String, String>,
}

/// A sidecar's lesswire identity together with the host key it trusts.
///
/// The identity is generated on first use and reused afterwards, so the host's
/// approval survives restarts; the pinned host key makes a replaced host visible
/// instead of silently trusted.
pub struct ClientState {
    store: FileStore,
    identity: Zeroizing<[u8; 64]>,
    scope: KeyScope,
    trusted_servers: std::collections::HashMap<String, String>,
}

impl ClientState {
    /// Load the state, generating and persisting a fresh identity when absent.
    pub async fn load(store: FileStore, scope: KeyScope) -> Result<Self, StateError> {
        if let Some(bytes) = store.load(MAX_STATE_SIZE).await? {
            let persisted: PersistedClientState =
                serde_json::from_slice(&bytes).map_err(|_| StateError::Malformed)?;
            if persisted.version != STATE_VERSION || persisted.scope != scope {
                return Err(StateError::Malformed);
            }
            let decoded = Zeroizing::new(
                hex::decode(&persisted.identity).map_err(|_| StateError::Malformed)?,
            );
            let identity: [u8; 64] = decoded
                .as_slice()
                .try_into()
                .map_err(|_| StateError::Malformed)?;
            validate_trusted_servers(&persisted.trusted_servers)?;
            return Ok(Self {
                store,
                identity: Zeroizing::new(identity),
                scope,
                trusted_servers: persisted.trusted_servers.clone(),
            });
        }

        let state = Self {
            store,
            identity: Identity::generate(scope)?.to_bytes(),
            scope,
            trusted_servers: std::collections::HashMap::new(),
        };
        state.persist().await?;
        Ok(state)
    }

    pub fn identity(&self) -> Result<Identity, StateError> {
        Ok(Identity::from_bytes(self.scope, self.identity.as_slice())?)
    }

    pub fn trusted_server(&self, endpoint_id: &str) -> Option<&str> {
        self.trusted_servers.get(endpoint_id).map(String::as_str)
    }

    pub fn has_trusted_servers(&self) -> bool {
        !self.trusted_servers.is_empty()
    }

    /// Pin the host key learned from a handshake for a stable endpoint ID.
    pub async fn set_trusted_server(
        &mut self,
        endpoint_id: String,
        bundle: String,
    ) -> Result<(), StateError> {
        validate_bundle(&bundle)?;
        let previous = self.trusted_servers.insert(endpoint_id.clone(), bundle);
        if let Err(error) = self.persist().await {
            match previous {
                Some(value) => self.trusted_servers.insert(endpoint_id, value),
                None => self.trusted_servers.remove(&endpoint_id),
            };
            return Err(error);
        }
        Ok(())
    }

    /// Forget the pinned host key so the next handshake pairs again.
    pub async fn reset_pairing(&mut self) -> Result<(), StateError> {
        let previous = std::mem::take(&mut self.trusted_servers);
        if let Err(error) = self.persist().await {
            self.trusted_servers = previous;
            return Err(error);
        }
        Ok(())
    }

    async fn persist(&self) -> Result<(), StateError> {
        let persisted = PersistedClientState {
            version: STATE_VERSION,
            identity: hex::encode(self.identity.as_slice()),
            scope: self.scope,
            trusted_servers: self.trusted_servers.clone(),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&persisted)?);
        self.store.save(&bytes).await?;
        Ok(())
    }
}

fn validate_trusted_servers(
    trusted_servers: &std::collections::HashMap<String, String>,
) -> Result<(), StateError> {
    trusted_servers
        .values()
        .try_for_each(|bundle| validate_bundle(bundle))
}

fn validate_bundle(bundle: &str) -> Result<(), StateError> {
    let parsed = PublicKeyBundle::parse(bundle).ok_or(StateError::Malformed)?;
    (parsed.as_str() == bundle)
        .then_some(())
        .ok_or(StateError::Malformed)
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

        let mut state = ClientState::load(store.clone(), KeyScope::App)
            .await
            .unwrap();
        let identity_bundle = state.identity().unwrap().public_key_bundle();
        let server_bundle = Identity::generate(KeyScope::Core)
            .unwrap()
            .public_key_bundle();
        assert!(state.trusted_server("core").is_none());

        state
            .set_trusted_server("core".into(), server_bundle.clone())
            .await
            .unwrap();

        let reloaded = ClientState::load(store.clone(), KeyScope::App)
            .await
            .unwrap();
        assert_eq!(
            reloaded.identity().unwrap().public_key_bundle(),
            identity_bundle
        );
        assert_eq!(
            reloaded.trusted_server("core"),
            Some(server_bundle.as_str())
        );

        let mut reloaded = reloaded;
        reloaded.reset_pairing().await.unwrap();
        assert!(
            ClientState::load(store, KeyScope::App)
                .await
                .unwrap()
                .trusted_server("core")
                .is_none()
        );
    }

    #[tokio::test]
    async fn rejects_noncanonical_trusted_server_bundles() {
        let directory = tempfile::tempdir().unwrap();
        let store = FileStore::at(directory.path().join("state.json"));
        let mut state = ClientState::load(store, KeyScope::App).await.unwrap();

        let error = state
            .set_trusted_server("core".into(), "not-a-bundle".into())
            .await
            .unwrap_err();
        assert!(matches!(error, StateError::Malformed));
    }

    #[tokio::test]
    async fn rejects_persisted_invalid_trusted_server_bundles() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        let state = serde_json::json!({
            "version": STATE_VERSION,
            "identity": hex::encode(Identity::generate(KeyScope::App).unwrap().to_bytes()),
            "scope": "app",
            "trustedServers": { "core": "not-a-bundle" },
        });
        tokio::fs::write(&path, serde_json::to_vec(&state).unwrap())
            .await
            .unwrap();

        let error = match ClientState::load(FileStore::at(path), KeyScope::App).await {
            Ok(_) => panic!("an invalid trusted server bundle should be rejected"),
            Err(error) => error,
        };
        assert!(matches!(error, StateError::Malformed));
    }

    #[tokio::test]
    async fn rejects_malformed_state_instead_of_replacing_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vhid-state.json");
        let old_state = serde_json::json!({
            "version": 2,
            "identity": hex::encode(Identity::generate(KeyScope::App).unwrap().to_bytes()),
            "scope": "app",
            "trustedServers": {},
        });
        tokio::fs::write(&path, serde_json::to_vec(&old_state).unwrap())
            .await
            .unwrap();
        let error = ClientState::load(FileStore::at(path.clone()), KeyScope::App)
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
