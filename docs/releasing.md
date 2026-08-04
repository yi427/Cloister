# Release and image versioning

Cloister keeps application releases, Profile compatibility, image contents,
and development channels explicit. They are related, but they are not the same
version boundary.

## Version sources

- `Cargo.toml` is the source of truth for the Cloister CLI release version.
- A release image uses the same exact version as the CLI that selects it.
- `schema_version` changes only when the public Profile contract becomes
  incompatible; it does not follow every CLI release.
- Node.js, Rust, Codex, and Claude Code versions remain independently pinned in
  `images/rust-node/Containerfile`. Updating one of them requires a new
  Cloister release, but its upstream version does not become the Cloister
  version.

The planned `cloister init` command will derive its default release image from
the compiled CLI version:

```text
ghcr.io/yi427/cloister:<CARGO_PKG_VERSION>
```

For example, Cloister CLI `0.1.0` selects
`ghcr.io/yi427/cloister:0.1.0`.

## Image tags

The image workflow publishes these channels:

| Tag | Meaning | Mutable |
| --- | --- | --- |
| `main` | Latest successful image build from `main` | Yes |
| `sha-<full-commit>` | Image built from one exact Git commit | No |
| `X.Y.Z` | Image for one exact Cloister release | No |
| `X.Y` | Latest patch image in one minor release line | Yes |

Cloister does not publish `latest`. Profiles created for normal use must select
an exact `X.Y.Z` release, never `main` or the moving `X.Y` channel. Published
`X.Y.Z` and `sha-<full-commit>` tags must never be moved or overwritten; fix a
bad image by publishing a new patch release.

The package is private on first publication. A maintainer must explicitly make
`ghcr.io/yi427/cloister` public before documenting it as anonymously
pullable. Public package visibility cannot later be changed back to private.

## Release procedure

1. Choose the next semantic version.
2. Update `package.version` in `Cargo.toml`; let Cargo update the root package
   entry in `Cargo.lock`.
3. Deliberately update pinned guest-tool versions in
   `images/rust-node/Containerfile` when the release requires them.
4. Update user-facing Profile examples to the exact release image when that
   image becomes the supported default.
5. Run `make format` and `make verify`.
6. Commit the reviewed release changes on `main` and push them.
7. Create and push an annotated `vX.Y.Z` Git tag.
8. Confirm that GitHub Actions published `X.Y.Z`, `X.Y`, and the immutable
   `sha-<full-commit>` image tags, then verify the exact image through Apple
   `container` before changing defaults used by `init`.

Example for version `0.1.0`:

```sh
git tag -a v0.1.0 -m "Release Cloister 0.1.0"
git push origin v0.1.0
```

The publish workflow rejects a release tag whose version does not exactly
match the root `cloister` package version in `Cargo.toml`.

## Local development image

`make image` builds `cloister:dev`. This local tag is intentionally separate
from GHCR release tags and must not appear in Profiles intended for end users.
