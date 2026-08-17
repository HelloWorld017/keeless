# @keeless/passkey-windows

Windows WebAuthn Plugin Passkey Manager sidecar for Keeless. It is a classic
out-of-process COM server which verifies Windows operation signatures and
Windows Hello verification before sending a ceremony to the authenticated
Keeless desktop host. The desktop host remains the only process that unlocks
the database, prompts for consent, and accesses passkey private keys.

The provider intentionally does not use Windows credential metadata cache APIs.
Browsers therefore require an explicit Keeless selection and conditional/autofill
discovery returns `NTE_NOT_FOUND` without starting Keeless or prompting.

## ABI Source

`src/sdk_bindings.rs` is a checked-in, ABI-reviewed subset derived from
`microsoft/webauthn` commit `ef82c157125a0490e05f6ea82a7adb1b8e1bad08`.
It never reads a developer Windows SDK or the network during a normal build.
The MIT-licensed ABI inputs are:

- `pluginauthenticator.idl` and `pluginauthenticator.h` for the COM callback ABI
- `webauthnplugin.h` for plugin management, CTAP-CBOR codec, and Hello APIs
- `webauthn.h` for the nested request and response structures

To update the binding, download exactly that commit, compare the four input
files above with the source header notice, regenerate the ABI subset with a
current `bindgen` in a Windows SDK build environment, and review the resulting
layout changes before committing. The normal Cargo build only compiles the
checked-in Rust source.

The executable dynamically resolves every required export from the System32
copy of `webauthn.dll`, so unsupported Windows versions return an explicit
unsupported result instead of failing during process loading.

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

The Windows-native CNG tests must be run before release. Browser/VM integration
testing and installer staging are owned by the desktop package, not this crate.

`keeless_passkey_ctap::response::authenticator_info_cbor` supplies the raw
`authenticatorGetInfo` CBOR used by the registration adapter; it does not
include CTAPHID's leading status byte.

## Commands

The installed sidecar accepts:

```text
keeless-passkey-windows --enable
keeless-passkey-windows --disable
keeless-passkey-windows doctor
keeless-passkey-windows reset-pairing
```

`--enable` and `--disable` only add or remove the Windows provider registration;
they never alter KDBX passkeys. `doctor` verifies dynamic API availability and
reports the provider state. COM activation uses `-PluginActivated -Embedding`.

For development only, `-PluginActivated --desktop C:\absolute\keeless.exe`
starts an explicit desktop executable on demand. Installed activation derives
the desktop executable from the sidecar's fixed installation directory; it does
not use `PATH` or user-writable configuration.

## Packaging

The Windows NSIS installer retains the classic `COM_CLASS_ID` registration in
the 64-bit `HKLM\\Software\\Classes\\CLSID` view. It also registers a minimal
external-location MSIX whose application is the installed sidecar. This grants
the sidecar the package identity required by `WebAuthNPluginAddAuthenticator`;
the MSIX does not own COM registration.

`build/msix/AppxManifest.xml` declares the sparse package. `build.rs` embeds
the corresponding Win32 application manifest in the sidecar so direct COM
activation receives the registered package identity. Its publisher must match
the package manifest exactly. Vite's `binary:` plugin emits both artifacts from
`target` into `dist/main/assets`; electron-builder packages them directly.

On a Windows release build agent, set `MSIX_PUBLISHER` to the exact subject of
the SignPath certificate and set an advancing four-part `MSIX_VERSION`, then
run `pnpm build`. This produces an unsigned MSIX and sidecar in `dist`. SignPath
must sign both artifacts before `pnpm dist` runs electron-builder.
The sparse package uses neutral architecture because the executable resides in
the installer-provided external location.

For development, `pnpm --filter @keeless/passkey-windows build:msix:dev`
creates or reuses a local `CN=Keeless Development` code-signing certificate,
trusts its public portion in the current user's Trusted People store, and
self-signs the debug MSIX. The development build path invokes this command
before Vite emits the MSIX. The private key remains in the Windows certificate
store and is never committed.

The sidecar remains in `$INSTDIR\\resources\\bin`; `--enable` and `--disable`
must run after the external package identity is registered. Uninstall disables
the provider, removes the current user's package identity, then removes the
classic COM registration.

`--disable` idempotently calls `WebAuthNPluginRemoveAuthenticator`. The
uninstaller invokes it before removing the identity package and leaves the
application installed if disabling fails.
