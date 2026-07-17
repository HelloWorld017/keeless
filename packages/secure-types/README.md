# secure-types

Keeless-vendored, byte-oriented fork based on `secure-types` 0.3.0:

- Upstream: <https://crates.io/crates/secure-types/0.3.0>
- Upstream repository: <https://github.com/greekfetacheese/secure-types>
- Upstream license: `MIT OR Apache-2.0`

The generic upstream allocator was replaced with a smaller API limited to byte
buffers, fixed-size byte arrays, and UTF-8 strings. On Linux, each value uses a
dedicated anonymous mapping and construction fails when `mlock` fails. The
mapping stays readable while alive; scoped access is enforced by the Rust API,
avoiding page-permission races between concurrent readers.

The `no_os` configuration provides zeroization on drop without memory locking
and is used for WebAssembly and non-Linux targets.
