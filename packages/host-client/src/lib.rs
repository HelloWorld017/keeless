//! Client side of the Keeless desktop host.
//!
//! Sidecar processes — the virtual HID daemon, the Windows plugin authenticator,
//! the native messaging host — reach the running desktop app through this crate:
//! [`ipc`] carries length-bounded messages over a current-user local endpoint, and
//! [`CoreClient`] wraps them in the authenticated, encrypted lesswire protocol.

pub mod client;
pub mod fs;
pub mod ipc;
pub mod state;

pub use client::{ClientError, CoreClient};
pub use state::{ClientState, FileStore};
