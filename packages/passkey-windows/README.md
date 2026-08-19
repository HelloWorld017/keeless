# @keeless/passkey-windows

Windows WebAuthn Plugin Passkey Manager sidecar for Keeless. It is a classic
out-of-process COM server which verifies Windows operation signatures and
Windows Hello verification before sending a ceremony to the authenticated
Keeless desktop host. The desktop host remains the only process that unlocks
the database, prompts for consent, and accesses passkey private keys.

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
