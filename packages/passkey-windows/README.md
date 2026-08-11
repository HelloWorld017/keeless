# @keeless/passkey-windows

Platform-independent ceremony primitives for the Windows WebAuthn Plugin
Passkey Manager. This package converts decoded Windows requests to the existing
Keeless Core operations, maps Core failures to browser-visible HRESULTs, owns
one active transaction, and persists the authenticated local IPC client state.

It intentionally does not register a Windows provider yet. The public Windows
Plugin headers do not specify the operation-signing key blob, signed request
bytes, or Hello signature format. A supported Windows 11 fixture must establish
that contract before a COM server can safely verify requests and be enabled.

`keeless_passkey_ctap::response::authenticator_info_cbor` supplies the raw
`authenticatorGetInfo` CBOR used by the later registration adapter; it does not
include CTAPHID's leading status byte.
