use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicI64, Ordering},
};

use super::transfer::Packet;
use super::*;
use zeroize::Zeroizing;

#[derive(Default)]
struct MemoryStore(Mutex<Option<Vec<u8>>>);
impl StateStore for MemoryStore {
    fn load(&self) -> WireFuture<'_, Result<Option<Vec<u8>>>> {
        Box::pin(async { Ok(self.0.lock().unwrap().clone()) })
    }
    fn save<'a>(&'a self, state: &'a [u8]) -> WireFuture<'a, Result<()>> {
        Box::pin(async move {
            *self.0.lock().unwrap() = Some(state.to_vec());
            Ok(())
        })
    }
}
struct Approval(AtomicBool);
impl ApprovalProvider for Approval {
    fn approve(&self, _: &str) -> WireFuture<'_, Result<bool>> {
        Box::pin(async { Ok(self.0.load(Ordering::Relaxed)) })
    }
}
struct TestClock(AtomicI64);
impl Clock for TestClock {
    fn now_millis(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }
    fn monotonic_millis(&self) -> u64 {
        self.now_millis() as u64
    }
}

fn transfer_response_id(bytes: &[u8], kind: u8) -> TransferId {
    assert_eq!(&bytes[..3], &[TRANSFER_MAGIC, 1, kind]);
    TransferId::from_bytes(&bytes[3..]).unwrap()
}

#[test]
fn upload_is_preallocated_finished_and_consumed_once() {
    let clock = Arc::new(TestClock(AtomicI64::new(0)));
    let registry = TransferRegistry::new(clock);
    let owner = TransferOwner::new("client");
    let response = registry
        .handle_packet(owner.clone(), &Packet::BeginUpload { size: 3 }.encode())
        .unwrap();
    let id = transfer_response_id(&response, 2);

    let response = registry
        .handle_packet(
            owner.clone(),
            &Packet::UploadChunk {
                id: id.clone(),
                offset: 0,
                bytes: b"abc",
            }
            .encode(),
        )
        .unwrap();
    assert_eq!(response[2], 4);
    registry
        .handle_packet(owner.clone(), &Packet::Finish { id: id.clone() }.encode())
        .unwrap();
    assert_eq!(&*registry.consume_upload(&owner, &id).unwrap(), b"abc");
    assert!(matches!(
        registry.consume_upload(&owner, &id),
        Err(TransferError::NotFound)
    ));
}

#[test]
fn transfer_expires_after_one_minute_without_valid_packets() {
    let clock = Arc::new(TestClock(AtomicI64::new(0)));
    let registry = TransferRegistry::new(clock.clone());
    let owner = TransferOwner::new("client");
    let response = registry
        .handle_packet(owner.clone(), &Packet::BeginUpload { size: 1 }.encode())
        .unwrap();
    let id = transfer_response_id(&response, 2);
    clock.0.store(TRANSFER_TTL_MS as i64, Ordering::Relaxed);
    registry.purge_expired();
    assert!(matches!(
        registry.consume_upload(&owner, &id),
        Err(TransferError::NotFound)
    ));
}

#[test]
fn downloads_are_bound_to_the_authenticated_owner() {
    let clock = Arc::new(TestClock(AtomicI64::new(0)));
    let registry = TransferRegistry::new(clock);
    let owner = TransferOwner::new("client-a");
    let id = registry
        .publish_download(owner.clone(), Zeroizing::new(b"abc".to_vec()))
        .unwrap();
    assert!(matches!(
        registry.handle_packet(
            TransferOwner::new("client-b"),
            &Packet::BeginDownload { id: id.clone() }.encode()
        ),
        Err(TransferError::OwnerMismatch)
    ));
    let response = registry
        .handle_packet(owner, &Packet::BeginDownload { id }.encode())
        .unwrap();
    assert_eq!(response[2], 6);
}

#[tokio::test]
async fn server_client_round_trip_persists_prompted_not_runtime_approvals() {
    let store = Arc::new(MemoryStore::default());
    let clock = Arc::new(TestClock(AtomicI64::new(10_000)));
    let client_identity = Identity::from_secrets([7; 32], [9; 32]);
    let runtime_bundle = Identity::from_secrets([11; 32], [13; 32]).public_key_bundle();
    let mut client = Client::new(client_identity, None, clock.clone()).unwrap();
    let mut server = Server::new(ServerHost {
        store: store.clone(),
        approval_provider: Arc::new(Approval(AtomicBool::new(true))),
        clock: clock.clone(),
        runtime_approved_clients: vec![runtime_bundle.clone()],
    })
    .await
    .unwrap();

    let handshake = client.handshake_frame().unwrap();
    let response = server
        .handle_frame(&handshake, |_, _| async { Ok::<_, ()>(None) })
        .await
        .unwrap()
        .unwrap();
    let trusted = client.accept_handshake(&response).unwrap().unwrap();
    assert_eq!(trusted, server.public_key_bundle());
    assert!(
        server
            .handle_frame(&handshake, |_, _| async { Ok::<_, ()>(None) })
            .await
            .unwrap()
            .is_none()
    );

    let request = client.encrypt(b"secret request").unwrap();
    let response = server
        .handle_frame(&request, |_owner, plaintext| async move {
            assert_eq!(&*plaintext, b"secret request");
            Ok::<_, ()>(Some(b"secret response".to_vec()))
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        &*client.decrypt(&response).unwrap().unwrap(),
        b"secret response"
    );
    assert!(client.decrypt(&response).unwrap().is_none());

    let state: serde_json::Value =
        serde_json::from_slice(store.0.lock().unwrap().as_ref().unwrap()).unwrap();
    assert!(
        state["approvedClientBundles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == client.public_key_bundle().as_str())
    );
    assert!(
        !state["approvedClientBundles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == runtime_bundle.as_str())
    );
}

#[tokio::test]
async fn dynamic_runtime_approval_is_validated_and_not_persisted() {
    let store = Arc::new(MemoryStore::default());
    let clock = Arc::new(TestClock(AtomicI64::new(10_000)));
    let identity = Identity::from_secrets([31; 32], [33; 32]);
    let mut client = Client::new(identity, None, clock.clone()).unwrap();
    let mut server = Server::new(ServerHost {
        store: store.clone(),
        approval_provider: Arc::new(Approval(AtomicBool::new(false))),
        clock,
        runtime_approved_clients: Vec::new(),
    })
    .await
    .unwrap();

    assert!(server.add_runtime_approval("invalid").is_err());
    server
        .add_runtime_approval(&client.public_key_bundle())
        .unwrap();
    let response = server
        .handle_frame(&client.handshake_frame().unwrap(), |_, _| async {
            Ok::<_, ()>(None)
        })
        .await
        .unwrap()
        .unwrap();
    client.accept_handshake(&response).unwrap();

    let state: serde_json::Value =
        serde_json::from_slice(store.0.lock().unwrap().as_ref().unwrap()).unwrap();
    assert!(
        state["approvedClientBundles"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rejects_weak_signing_and_noncanonical_bundles() {
    let weak = format!(
        "v1.{}.{}",
        URL_SAFE_NO_PAD.encode([0; 32]),
        URL_SAFE_NO_PAD.encode([9; 32])
    );
    assert!(PublicKeyBundle::parse(&weak).is_none());
    assert!(PublicKeyBundle::parse("v1.AA==.AA").is_none());
}

#[test]
fn identity_round_trips_without_debugging_secrets() {
    let identity = Identity::from_secrets([21; 32], [22; 32]);
    let bytes = identity.to_bytes();
    let restored = Identity::from_bytes(&bytes[..]).unwrap();
    assert_eq!(restored.public_key_bundle(), identity.public_key_bundle());
    assert_eq!(format!("{identity:?}"), "Identity([REDACTED])");
}
