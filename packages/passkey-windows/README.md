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

## Installation

The Windows NSIS installer registers the future COM local server under the
stable `COM_CLASS_ID` in the 64-bit `HKLM\\Software\\Classes\\CLSID` view. It
only does so when `keeless-passkey-windows.exe` is present at
`$INSTDIR\\resources\\bin`; the current package does not yet produce that
server. The installer owns machine-wide registry writes. The desktop app must
never self-register from an unelevated process.

The server's future `--disable` command must idempotently call
`WebAuthNPluginRemoveAuthenticator`. The uninstaller invokes it before removing
the COM class and leaves the application installed if disabling fails.
