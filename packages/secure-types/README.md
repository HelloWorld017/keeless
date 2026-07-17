# keeless_secure_types

Keeless-vendored, byte-oriented fork based on `secure-types` 0.3.0:

- Upstream: <https://crates.io/crates/secure-types/0.3.0>
- Upstream repository: <https://github.com/greekfetacheese/secure-types>
- Upstream license: `MIT OR Apache-2.0`

The generic upstream allocator was replaced with a smaller API limited to byte
buffers, fixed-size byte arrays, and UTF-8 strings. On Linux, each value uses a
dedicated anonymous mapping and construction fails when `mlock` fails. The
mapping stays readable while alive; scoped access is enforced by the Rust API,
avoiding page-permission races between concurrent readers.

On Windows, each value is padded to a 16-byte boundary and encrypted at rest
with `CryptProtectMemory` using `CRYPTPROTECTMEMORY_SAME_PROCESS`. Scoped access
decrypts a temporary copy, which is zeroized after use; mutable changes replace
the stored ciphertext only after reprotection succeeds. Access methods return
errors when Windows cannot protect or unprotect the value.

The `no_os` configuration provides zeroization on drop without OS-backed memory
protection and is used for WebAssembly and unsupported native targets.
