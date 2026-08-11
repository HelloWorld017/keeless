//! Shared ceremony logic for the Windows WebAuthn plugin adapter.
//!
//! The Windows COM boundary must verify the platform's operation signature
//! before using these helpers. Keeping the Core translation and transaction
//! state here lets that boundary stay narrowly unsafe and Windows-specific.

pub mod cancellation;
pub mod ceremony;
pub mod error;
pub mod session;
