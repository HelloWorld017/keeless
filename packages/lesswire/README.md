# @keeless/lesswire

Lesswire is Keeless's signed, encrypted message-frame protocol. The Rust crate
also includes a small, test-only CLI named `lesswire-debug`.

## Debug CLI

Build it with:

```sh
cargo build -p keeless_lesswire --bin lesswire-debug
```

Generate a secret identity and its public-key bundle:

```sh
lesswire-debug generate
```

```json
{ "identity": "<base64url-secret>", "publicKey": "v1.<Ed25519>.<X25519>" }
```

Recover the public key for an identity:

```sh
lesswire-debug public-key --identity "$IDENTITY"
```

Encrypt stdin. `--identity` is optional; omitting it creates a one-shot sender:

```sh
printf 'hello' |
  lesswire-debug encrypt --public-key "$RECIPIENT_PUBLIC_KEY" > frame.json
```

Decrypt a frame to raw stdout bytes:

```sh
lesswire-debug decrypt --identity "$RECIPIENT_IDENTITY" < frame.json
```

Normal decryption enforces lesswire's 500 ms timestamp window. Stored frames can
be inspected explicitly with:

```sh
lesswire-debug decrypt --identity "$RECIPIENT_IDENTITY" --allow-stale < frame.json
```

The identity value is secret key material. Passing it as an argument can expose
it to local process inspection and shell history, so this tool must only be used
for local testing. Decrypt trusts the sender public key embedded in the input
frame; it verifies the signature but does not independently authenticate that
sender. For `keeless-native-ui`, trust comes from reading the frame directly
from the stdout pipe of the child process that the host spawned.
