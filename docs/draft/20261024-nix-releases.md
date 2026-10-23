# Nix releases

The source branch's flake uses local sources for development. The `flakes` branch
contains a generated lightweight flake: the same Nix definitions and lock file,
the Cargo lock file, a version-only package manifest, and `release.json` with the
source commit and fixed-output hash. It contains no application sources.

## Release pipeline

1. The `Generate release flake` step in `build-nix.yaml` hashes the checked-out
   release commit's GitHub source archive and pins the cache URL to the package
   version's release.
2. CI builds that generated flake with import-from-derivation disabled. Cargo
   vendoring uses the included Cargo lock file instead of reading fetched sources
   during evaluation. CI and consumers therefore evaluate the same package.
3. The `Export output` step exports the output plus runtime dependencies not
   available from `cache.nixos.org`. Each exported path is signed, verified against
   the published public key, and written as a compressed NAR and signed narinfo.
4. The release job publishes the cache assets, then appends a commit to `flakes`.
   Its first commit has no parent from the source branch. Later commits preserve
   the lightweight branch's history without force pushes or additional tags.

The repository must have `NIX_CACHE_PRIVATE_KEY` configured as an Actions secret,
matching `keeless.nenw.dev-1:eKVDsvA/nRl6mi0y8of31nX8xyOKdtiWvMVh0y8HXqk=`.
Publication uses the workflow's `contents: write` token. The `flakes` branch must
allow that token to push.

## Local verification

Download the `nix-flake` artifact from the build workflow and unpack it into
`/tmp/keeless-release-flake`, then evaluate or build it locally:

```sh
nix eval --raw path:/tmp/keeless-release-flake#packages.x86_64-linux.default.drvPath \
  --option allow-import-from-derivation false
nix build path:/tmp/keeless-release-flake#keeless \
  --option allow-import-from-derivation false
```

The generation step downloads sources to compute their hash; consumers only need
those sources on a cache miss. A local
source checkout's output path need not match the generated release flake's path.

After publication, check substitution on a machine without the output installed:

```sh
nix build --dry-run --accept-flake-config github:HelloWorld017/keeless/flakes
```

The plan should fetch keeless rather than build it. For both package and module
installation, configure the cache as shown in the README. Keep the generated
flake's inputs unchanged for cache hits.

## Existing v0.1.0 assets

The original v0.1.0 narinfo omitted the cache signature because the exporter
filtered for a different key name. These workflow changes do not repair published
assets. The original output needs its narinfo re-exported with the matching private
key and uploaded to the existing release. Its output path is also distinct from
the newly generated flake's package, so repairing its signature alone does not
populate the cache for the new packaging definition.
