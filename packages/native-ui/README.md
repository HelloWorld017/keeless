# @keeless/native-ui

`keeless-native-ui` displays one native dialog and writes one encrypted response
to stdout. It is intended to be spawned by a trusted native host rather than by
the web renderer.

## CLI

```text
keeless-native-ui --public-key <lesswire-public-key-bundle> <kind> <args-json>
```

Examples:

```sh
keeless-native-ui --public-key "$KEY" password '{"mode":"unlock"}'
keeless-native-ui --public-key "$KEY" file '{"mode":"open","filters":[{"name":"KeePass database","extensions":["kdbx"]}]}'
keeless-native-ui --public-key "$KEY" connection '{"publicKey":"v1...","name":"Browser extension"}'
```

Supported requests:

```ts
type PasswordArgs = {
  mode: 'create' | 'unlock' | 'reveal' | 'save';
};

type FileArgs = {
  mode: 'open' | 'save';
  title?: string;
  filters?: Array<{ name: string; extensions: string[] }>;
  directory?: string;
  fileName?: string;
};

type ConnectionArgs = {
  publicKey: string;
  name?: string;
};
```

The public key must use the lesswire `v1.<Ed25519>.<X25519>` bundle format.
stdout contains one compact JSON-encoded `MessageFrame` followed by a newline.
The frame payload is one of:

```json
{"version":1,"kind":"password","status":"selected","result":{"password":"..."}}
{"version":1,"kind":"file","status":"selected","result":{"path":"/absolute/path"}}
{"version":1,"kind":"connection","status":"selected","result":{"allowed":true}}
{"version":1,"kind":"file","status":"cancelled"}
{"version":1,"kind":"password","status":"error","error":{"code":"ui_unavailable","message":"..."}}
```

Cancellation, denial, and UI errors are encrypted. Invalid CLI arguments cannot
produce a trusted encrypted response; they are written to stderr and exit with
code 2. Encryption or stdout failures exit with code 1.

The process creates a fresh lesswire sender identity for each response. The
parent may approve the `publicKey` in that frame only for this response because
the frame came directly from the stdout pipe of the child it spawned.

`--public-key` provides response confidentiality; it does not authenticate the
process that launched the dialog. The native host must spawn this binary itself
and keep stdin/stdout private. The host keeps the child's piped stdin open for
the duration of the request; EOF closes the helper when its parent exits.

## Password handling

The password editor uses a fixed-capacity `SecureBytes` allocation and does not
use egui's normal `TextEdit` undo history. Copy and cut are disabled, temporary
input events are zeroized after use, and plaintext JSON buffers are zeroized
after encryption. This reduces application-owned plaintext copies but cannot
remove copies created by the OS keyboard, IME, or clipboard stack. The masked
display also reveals password length. Passwords are limited to 4096 UTF-8 bytes.

The synchronous `rfd` API does not distinguish a native file-dialog backend
failure from user cancellation; both are returned as `status: "cancelled"`.
