# keeless_kdbx

Rust KeePass database library used by Keeless. The code was imported from
`keepass-rs` 0.2.0 and hardened for KDBX interoperability, data preservation,
untrusted input, and native/WebAssembly builds.

## Upstream Source

This crate was imported from the crates.io archive `keepass-rs` version
`0.2.0` because its declared Git repository was unavailable.

- Package: `https://crates.io/crates/keepass-rs/0.2.0`
- Archive SHA-256: `077bb10f8c31bc2edcc8761e6f0ae21916d88805725bc8ff76912f0390049924`
- Imported license: `MIT OR MulanPSL-2.0`

The implementation has since diverged to correct KDBX compatibility,
security, data-preservation, and WebAssembly issues.

## Formats

- KDB v1 read/write
- KDBX 3.1 read/write
- KDBX 4.0 and 4.1 read
- KDBX 4.x write

KDBX4 output is covered by an interoperability test using the independent
`keepass` parser. Well-formed XML elements that are not understood by this
crate are retained under their semantic parent and written back when saving;
malformed known structures and values are rejected explicitly.

## Usage

```rust
use keeless_kdbx::{open_database, save_database, CompositeKey};

let key = CompositeKey::new().with_password(b"password")?;
let database = open_database(std::fs::File::open("database.kdbx")?, &key)?;
save_database(&mut std::fs::File::create("output.kdbx")?, &database, &key)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `diagnose_database` when callers need structured per-stage failure details.
The `kdbx-debug` workspace package provides a human-readable and JSON CLI on
top of this API.
