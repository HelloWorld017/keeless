# Upstream KDBX test fixtures

These files are synthetic test data vendored from upstream interoperability
suites. Tests verify each SHA-256 digest before parsing so an accidental fixture
change fails explicitly.

## KDBXWeb

- Repository: <https://github.com/keeweb/kdbxweb>
- Commit: `7601e45d2a437f35a7fa3d767164eab60bc05d42`
- License: MIT, preserved in `kdbxweb/LICENSE`

| Local file | Upstream path | Git blob | SHA-256 | Test password |
| --- | --- | --- | --- | --- |
| `kdbxweb/EmptyPass.kdbx` | `resources/EmptyPass.kdbx` | `54179987330be4ef3e1badfe42ef37adab1b0514` | `4af2c576a50caa7d3104817453abe7843416e362548c2af3e2721976a434d49e` | empty string |
| `kdbxweb/KDBX4.1.kdbx` | `resources/KDBX4.1.kdbx` | `a04df49e334e9686baa0231b573a3e2140d630f7` | `189535d9f2c097756f1b93f9985727643097d56ce07243a871f27a03268f2906` | `test` |
| `kdbxweb/cyrillic.kdbx` | `resources/cyrillic.kdbx` | `3538a168d2941744abf039e63828e95a6c6f8934` | `552501ed6c218c37d58198750e924db6ab6ee1858076e7851228387b44af70d5` | `пароль` |

## KeePassXC

- Repository: <https://github.com/keepassxreboot/keepassxc>
- Commit: `6a534f38c61295d60b0e937739a12511e76dbfeb`
- License: GPL-2.0-or-later or GPL-3.0-or-later, preserved in
  `keepassxc/COPYING`

| Local file | Upstream path | Git blob | SHA-256 | Test password |
| --- | --- | --- | --- | --- |
| `keepassxc/BrokenHeaderHash.kdbx` | `tests/data/BrokenHeaderHash.kdbx` | `6c4c43991479aab7c4fbcbd0ec7168edae72c8a4` | `681e9117b297ee7be9d569f066b07f2a361d1bf0aae49074033022425f267a00` | empty string |
| `keepassxc/Compressed.kdbx` | `tests/data/Compressed.kdbx` | `1f8ec2de6871a80cc65cf7e42e38efb5881a9ef3` | `392b089bf3f17f7e507dc2d97584493f1b709f6989b37c2b964bfc8b21e994b8` | empty string |
| `keepassxc/Format400.kdbx` | `tests/data/Format400.kdbx` | `1a877508e838a68870fc8ba6420b0e6b55aead72` | `4f23b3f5f6c71209e135dab92d0f06034315d3f70e3e2ef3ec3a77ff37b49435` | `t` |
| `keepassxc/NonAscii.kdbx` | `tests/data/NonAscii.kdbx` | `8ebaac249af5aaf9a47a306bc8f3e1d5ef3be3fd` | `e8a69ba7e0cbd86a98b9a9d8fb6df8deecb052a4a3da842b200776434cb760e4` | `Δöض` |
| `keepassxc/ProtectedStrings.kdbx` | `tests/data/ProtectedStrings.kdbx` | `bb50c03fbfe9afbc99e889c1abc87cec00ed1982` | `15efb80917ffff30173e75d5bb53c270a10076b26a90f4a80d2071b71f318d8c` | `masterpw` |

The credentials above are intentionally public upstream test credentials, not
production secrets. Update a fixture only by pinning a new commit and updating
its blob hash, SHA-256 digest, expected contents, and license record together.
