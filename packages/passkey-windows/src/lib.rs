//! Shared ceremony logic for the Windows WebAuthn plugin adapter.
//!
//! The Windows COM boundary must verify the platform's operation signature
//! before using these helpers. Keeping the Core translation and transaction
//! state here lets that boundary stay narrowly unsafe and Windows-specific.

/// Stable CLSID registered by the per-machine NSIS installer.
///
/// Do not change after a release: Windows WebAuthn registration and the classic
/// COM registration in `packages/desktop/build/installer.nsh` both use it.
pub const COM_CLASS_ID: &str = "{13ABEFF0-71C5-49E3-9F2F-C207A28CDB9D}";

pub mod cancellation;
pub mod ceremony;
pub mod credential_cache;
pub(crate) mod diagnostics;
pub mod error;
pub mod session;

#[cfg(windows)]
pub mod api;
#[cfg(windows)]
pub mod authenticator;
#[cfg(windows)]
pub mod com;
#[cfg(windows)]
pub mod package_identity;
#[cfg(windows)]
pub(crate) mod provider;
#[cfg(windows)]
pub mod registration;
#[cfg(windows)]
pub mod sdk_bindings;
#[cfg(windows)]
pub mod verify;

#[cfg(windows)]
pub fn main(arguments: impl IntoIterator<Item = std::ffi::OsString>) -> Result<(), String> {
    com::main(arguments)
}
