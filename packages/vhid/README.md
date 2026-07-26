# @keeless/vhid

`keeless-vhid` serves the passkeys in your Keeless database to any application
that speaks WebAuthn on Linux. It creates a virtual HID device, so browsers see
Keeless as an ordinary security key with no extension or patched build.

## Requirements

- Linux with the `uhid` driver (`CONFIG_UHID`), which mainstream distributions ship
- Write access to `/dev/uhid`, which is root-owned by default
- The Keeless desktop app running, since it owns the database

## Setup

`/dev/uhid` needs a udev rule before a desktop session can use it. Print the
commands with:

```sh
keeless-vhid setup
```

They install a rule tagging `uhid` for session access, make sure the driver is
loaded at boot, and reload udev. Run them as root, then log out and back in so
the new access applies. Check the result any time with:

```sh
keeless-vhid doctor
```

which reports whether the device is usable, whether the daemon has been approved
by the app, and whether the app is reachable.

## Running

```sh
keeless-vhid run --native-ui /path/to/keeless-native-ui
```

`--native-ui` may be omitted when the helper is on `PATH`. The daemon serves
requests until it is stopped; only one instance runs at a time.

The first request asks the desktop app to approve this daemon, the same dialog
any new local client gets. The app's key is then pinned, so a replaced app is
visible rather than silently trusted. `keeless-vhid reset-pairing` forgets it.

## What a ceremony looks like

1. A site asks the browser for a passkey, and the browser sends CTAP2 over the
   virtual device.
2. If the database is locked, the app prompts for the master password. The daemon
   keeps the browser informed while that happens.
3. The daemon shows its own prompt naming the site, and — when several passkeys
   match — the accounts to choose between.
4. The chosen credential signs, in the app's process. The private key never
   leaves it.

If the browser withdraws the request, the prompt closes with it.

## Limitations

- The app must be running. Requests that arrive while it is closed are refused
  with a CTAP error rather than taking the device down, so the browser can fall
  back to another authenticator.
- Signature counters stay at zero, which is the usual choice for passkeys that
  sync between devices.
- There is no PIN. User verification is the database's own password plus the
  consent prompt, and `authenticatorGetInfo` reports no `clientPin` option.
- No extensions are implemented; `hmac-secret` and `prf` requests are ignored
  rather than refused, so a ceremony that merely prefers them still completes.

## Testing against a real device

The device-level test is skipped unless you ask for it, because it needs
`/dev/uhid`:

```sh
cargo test -p keeless_vhid -- --ignored
```

For a manual check, `fido2-token -L` from libfido2 lists the device, and
`fido2-cred`/`fido2-assert` drive full ceremonies without a browser.
