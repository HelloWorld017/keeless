# @keeless/passkey-windows

Platform-independent ceremony primitives for the Windows WebAuthn Plugin
Passkey Manager. The current crate converts decoded Windows requests to existing
Keeless Core operations, maps Core failures to browser-visible HRESULTs, owns
one active transaction, and persists authenticated local IPC client state. The
COM server and provider registration are not implemented yet.

## ABI Source

The future Windows adapter pins `microsoft/webauthn` commit
`ef82c157125a0490e05f6ea82a7adb1b8e1bad08` and commits generated bindings.
The generator consumes all of these MIT-licensed inputs:

- `pluginauthenticator.idl` and `pluginauthenticator.h` for the COM callback ABI
- `webauthnplugin.h` for plugin management, CTAP-CBOR codec, and Hello APIs
- `webauthn.h` for the nested request and response structures

The executable dynamically resolves required exports from `webauthn.dll`, so
unsupported Windows versions return an explicit unsupported result instead of
failing during process loading.

## Ceremony Contract

Until Windows VM testing is available, the implementation follows the observed
behavior of `yusei36/KeePassPasskey` commit
`08a3e0b13b81ee55929c4e9e0895e7197118b3b6`. That GPL-3.0 source is never
copied; Keeless independently implements only the public ABI and this contract:

- operation, cancellation, and Windows Hello v1 signatures cover the exact
  `pbEncodedRequest` bytes;
- operation and UV keys are fetched from the platform for each callback and
  imported as CNG `GenericPublicBlob` values;
- signatures use SHA-256; RSA-PSS is attempted before RSA-PKCS#1 v1.5, and EC
  signatures use CNG ECDSA verification;
- unsupported CNG key types, malformed blobs, and malformed pointer/length
  pairs fail before SDK decode, Windows Hello, or Core IPC;
- `WebAuthNPluginPerformUserVerification` v1 is used, rather than the v2 custom
  buffer API.

This contract must be covered by Windows-native CNG and browser integration
tests before the provider is enabled for release.

`keeless_passkey_ctap::response::authenticator_info_cbor` supplies the raw
`authenticatorGetInfo` CBOR used by the registration adapter; it does not
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
