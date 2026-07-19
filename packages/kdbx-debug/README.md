# kdbx-debug

`kdbx-debug` identifies the exact stage at which a KDBX 3.1, 4.0, or 4.1
database fails to open.

```sh
cargo run -p keeless_kdbx_debug -- vault.kdbx
cargo run -p keeless_kdbx_debug -- vault.kdbx --key-file vault.keyx
cargo run -p keeless_kdbx_debug -- vault.kdbx --json
cargo run -p keeless_kdbx_debug -- vault.kdbx --dump-xml vault-debug.xml
```

The password is read from a hidden prompt and is never accepted as a command
line argument. Use `--no-password --key-file ...` for a key-file-only database.
Standard raw, hex, XML v1, and XML v2 KeePass key files are supported.

`--dump-xml` writes the outer-decrypted XML to a new file. Existing files are
not overwritten, and Unix files are created with mode `0600`. Values marked
`Protected="True"` remain encrypted, but other database metadata can be plain
text and should still be handled as sensitive data.
