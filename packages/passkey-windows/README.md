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

The Windows NSIS installer deploys an external-location MSIX identity package
alongside the classic installation. The package manifest owns the stable
`COM_CLASS_ID` registration through `windows.comServer`; it replaces direct
`HKLM\\Software\\Classes\\CLSID` writes and causes COM activation to include
package identity. That identity is required by `WebAuthNPluginAddAuthenticator`.

The manifest template and packaging scripts are in
`packages/desktop/build/passkey-identity`. Before each Windows release:

1. Build an unsigned MSIX per target architecture with
   `build-identity-package.ps1`. Its publisher must exactly match the subject
   of the release signing certificate, and the four-part version must advance
   on updates.
2. Submit each MSIX to SignPath for package signing. Keep the package name
   `dev.nenw.keeless.passkey` and the publisher stable after the first release;
   changing either changes the package family.
3. Use `stage-signed-identity-package.ps1` to copy the signed, matching
   architecture package to `packages/desktop/resources/passkey-windows/` before
   building the corresponding NSIS installer. The staging script requires the
   `AppxSignature.p7x` produced by package signing.

For example, the pre-signing build step on a Windows build agent is:

```powershell
./packages/desktop/build/passkey-identity/build-identity-package.ps1 `
  -Publisher 'CN=Keeless' `
  -Version 0.0.0.0 `
  -Architecture x64 `
  -OutputPath .\artifacts\keeless-passkey-identity-x64.msix
```

`CN=Keeless` is only an example: production builds must use the exact SignPath
certificate subject. An unsigned package is useful only as SignPath input; the
NSIS installer never uses `-AllowUnsigned` and Windows rejects it.

The per-machine installer stages the signed package with its external location
set to `$INSTDIR`, then provisions it for all users. The sidecar remains in
`$INSTDIR\\resources\\bin`; `--enable` and `--disable` must run after package
identity is installed. Uninstall disables the provider before removing the
provisioned identity package and its manifest-provided COM registration.

`--disable` idempotently calls `WebAuthNPluginRemoveAuthenticator`. The
uninstaller invokes it before removing the identity package and leaves the
application installed if disabling fails.
