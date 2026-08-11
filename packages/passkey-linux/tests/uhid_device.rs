//! Exercises the daemon against a real virtual HID device.
//!
//! Ignored by default: it needs a kernel with `CONFIG_UHID` and write access to
//! `/dev/uhid`, which `keeless-passkey-linux setup` explains how to arrange. Run it with
//! `cargo test -p keeless_passkey_linux -- --ignored` on a Linux desktop.

#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::time::Duration;

use keeless_passkey_linux::authenticator::Authenticator;
use keeless_passkey_linux::session::Session;

/// The USB identifiers `uhid.rs` creates the device with.
const DEVICE_UEVENT_ID: &str = "1209:00005031";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires /dev/uhid"]
async fn a_host_can_initialize_a_channel_and_read_the_authenticator_info() {
    let (shutdown, stop) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(async move {
        let session = Session::load().await.expect("load the daemon state");
        keeless_passkey_linux::daemon::run(Authenticator::new(session), async {
            let _ = stop.await;
        })
        .await
        .expect("the daemon runs");
    });

    let node = tokio::task::spawn_blocking(find_hidraw_node)
        .await
        .expect("the search finishes")
        .expect("the virtual device appears as a hidraw node");

    let exchange = tokio::task::spawn_blocking(move || {
        let mut device = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&node)
            .expect("open the hidraw node");

        let nonce = [1_u8, 2, 3, 4, 5, 6, 7, 8];
        device
            .write_all(&report(0xffff_ffff, 0x86, &nonce))
            .expect("send INIT");
        let response = read_report(&mut device);
        assert_eq!(&response[7..15], nonce, "INIT echoes the nonce");
        let channel = u32::from_be_bytes(response[15..19].try_into().unwrap());
        assert_ne!(channel, 0);
        assert_ne!(channel, 0xffff_ffff);
        assert_eq!(response[19], 2, "CTAPHID protocol version");
        assert_eq!(response[23] & 0x04, 0x04, "CBOR is advertised");

        device
            .write_all(&report(channel, 0x90, &[0x04]))
            .expect("send getInfo");
        let response = read_report(&mut device);
        assert_eq!(
            &response[..4],
            channel.to_be_bytes(),
            "answered on our channel"
        );
        assert_eq!(response[4], 0x90, "a CBOR response");
        assert_eq!(response[7], 0x00, "CTAP2 success");
        assert_eq!(response[8] & 0xe0, 0xa0, "the payload is a CBOR map");
    });
    exchange.await.expect("the exchange completes");

    let _ = shutdown.send(());
    tokio::time::timeout(Duration::from_secs(5), daemon)
        .await
        .expect("the daemon stops")
        .expect("the daemon exits cleanly");
}

/// A 65-byte hidraw write: the report ID, then the CTAPHID packet.
fn report(channel: u32, command: u8, payload: &[u8]) -> Vec<u8> {
    let mut buffer = vec![0_u8; 65];
    buffer[1..5].copy_from_slice(&channel.to_be_bytes());
    buffer[5] = command;
    buffer[6..8].copy_from_slice(&(payload.len() as u16).to_be_bytes());
    buffer[8..8 + payload.len()].copy_from_slice(payload);
    buffer
}

fn read_report(device: &mut std::fs::File) -> [u8; 64] {
    let mut response = [0_u8; 64];
    device.read_exact(&mut response).expect("read a report");
    response
}

/// Poll for the hidraw node the kernel creates for our virtual device.
fn find_hidraw_node() -> Option<String> {
    for _ in 0..50 {
        let found = std::fs::read_dir("/sys/class/hidraw")
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .find(|name| {
                std::fs::read_to_string(format!("/sys/class/hidraw/{name}/device/uevent"))
                    .is_ok_and(|uevent| uevent.contains(DEVICE_UEVENT_ID))
            });
        if let Some(name) = found {
            return Some(format!("/dev/{name}"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}
