# @keeless/passkey-linux

`keeless-passkey-linux` serves the passkeys in your Keeless database to any application
that speaks WebAuthn on Linux. It creates a virtual HID device, so browsers see
Keeless as an ordinary security key with no extension or patched build.

## Requirements

- Linux with the `uhid` driver (`CONFIG_UHID`), which mainstream distributions ship
- Membership of a group with write access to `/dev/uhid`, which `setup` explains
- The Keeless desktop app running, since it owns the database

## Setup

`/dev/uhid` needs a udev rule before a desktop session can use it. Print the
commands with:

```sh
keeless-passkey-linux setup
```

Read them before running them. Write access to `/dev/uhid` is the ability to
create virtual input devices of any kind — a keyboard among them — so whoever
holds it can type into your session. The printed rule therefore grants it to a
`keeless-uhid` group you join, rather than to every user who logs in at the
console. Run the commands as root, then log out and back in. Check the result
any time with:

```sh
keeless-passkey-linux doctor
```

which reports whether the device is usable, whether the daemon has been approved
by the app, and whether the app is reachable.

## Running

```sh
keeless-passkey-linux run
```

The consent dialog helper is expected beside the `keeless-passkey-linux` executable;
`--native-ui` names it elsewhere, and must be an absolute path. It is never
searched for on `PATH`, because whatever answers that prompt decides which
signatures get made. The daemon serves requests until it is stopped; only one
instance runs at a time.

The first request asks the desktop app to approve this daemon, the same dialog
any new local client gets. The app's key is then pinned, so a replaced app is
visible rather than silently trusted. `keeless-passkey-linux reset-pairing` forgets it.

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

Browsers also ask, without prompting anyone, which of a site's credentials this
authenticator holds — that is how they decide whether to offer Keeless at all.
Those answers carry no user-presence flag, so a relying party rejects them, and
they name no accounts. They do require the database to be unlocked.

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
cargo test -p keeless_passkey_linux -- --ignored
```

For a manual check, `fido2-token -L` from libfido2 lists the device, and
`fido2-cred`/`fido2-assert` drive full ceremonies without a browser.
