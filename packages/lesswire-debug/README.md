# lesswire-debug

`lesswire-debug` is a local testing CLI for the Lesswire signed and encrypted
message-frame protocol.

Build it with:

```sh
cargo build -p keeless_lesswire_debug
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

The debug CLI accepts stored frames regardless of their timestamp. Normal
lesswire clients and servers still enforce the 500 ms timestamp window.

The identity value is secret key material. Passing it as an argument can expose
it to local process inspection and shell history, so this tool must only be used
for local testing. Decrypt trusts the sender public key embedded in the input
frame; it verifies the signature but does not independently authenticate that
sender. For `keeless-native-ui`, trust comes from reading the frame directly
from the stdout pipe of the child process that the host spawned.
