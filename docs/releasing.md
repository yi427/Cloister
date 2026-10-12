# Release and image versioning

Cloister keeps application releases, Profile compatibility, image contents,
and development channels explicit. They are related, but they are not the same
version boundary.

## Version sources

- `Cargo.toml` is the source of truth for the Cloister CLI release version.
- A release image uses the same exact version as the CLI that selects it.
- `schema_version` changes when the public Profile contract or authorization
  semantics need a new version; it does not follow every CLI release. Version
  0.4.0 introduces V8 while retaining V7 allowlist behavior. V8 requires an
  explicit Host Exec mode.
- Node.js and Rust remain pinned in `images/rust-node/Containerfile`.
- Codex and Claude Code are installed on first use and updated independently.
  Agent updates do not require a Cloister image release. Record the exact Agent
  versions used for release acceptance; CLI/image pairing does not certify all
  future Agent versions.

The `cloister init` command derives its default release image from the compiled
CLI version:

```text
ghcr.io/yi427/cloister:<CARGO_PKG_VERSION>
```

For example, Cloister CLI `0.4.1` selects
`ghcr.io/yi427/cloister:0.4.1`.

The CLI enforces this pair for official release images. An older or newer
official `X.Y.Z` image fails closed before agent startup, including `--dry-run`
plans.

## Choosing a release version

Use a patch release for a focused compatible bug fix. Use a minor release for
new capabilities or authorization semantics: `0.4.0` introduces explicit Host
Exec modes in Profile V8 and retains V7 allowlist behavior. Update the CLI
version, lockfile, exact image reference, and release documentation together during
release preparation; a development fix does not require an immediate version
bump. Never replace the already published `v0.3.0` tag or `0.3.0` image.

The latest published release is
[0.4.1](https://github.com/yi427/Cloister/releases/tag/v0.4.1).
See the [0.4.1 notes](releases/v0.4.1.md) for changes and upgrade instructions.

## CLI distribution boundary

The 0.4.x CLI is distributed through the official `yi427/tap` Homebrew tap.
The Formula downloads the immutable release archive, verifies its SHA-256,
installs the Apple `container` runtime dependency, and compiles Cloister with
Homebrew's Rust toolchain.

```sh
brew install yi427/tap/cloister
```

Direct installation from the exact Git tag remains supported:

```sh
cargo install --locked \
  --git https://github.com/yi427/Cloister.git \
  --tag v0.4.1 \
  cloister
```

The release does not attach prebuilt binaries. GitHub Releases provide reviewed
notes and source archives; the annotated Git tag remains the source of truth.
The Homebrew Formula is downstream distribution metadata and must never move or
replace a Cloister tag.

The Formula is maintained in `yi427/homebrew-tap`. For every release, update
its URL and SHA-256, then run:

```sh
brew audit --strict --online yi427/tap/cloister
brew install --build-from-source yi427/tap/cloister
brew test yi427/tap/cloister
```

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

The `main` build publishes `sha-<full-commit>`. A release-tag build publishes
only the semantic-version tags so its release metadata does not overwrite the
existing SHA image from that commit.

At runtime, a full `sha-<40-hex>` tag is accepted as an immutable testing image
with a warning. Official moving tags (`main`, `latest`, and `X.Y`) are rejected.
Local and third-party image references remain available for development with
an unverified-compatibility warning.

The `ghcr.io/yi427/cloister` package is public. Every release must verify
anonymous manifest access before the install command is documented as ready.
GitHub package visibility cannot later be changed back to private.

## Release gates

Before creating a release tag, all of the following must be true:

- the working tree is clean and local `main` matches `origin/main`;
- `Cargo.toml` and the root package entry in `Cargo.lock` contain the intended
  version;
- `examples/profile.toml` selects the matching exact `X.Y.Z` image;
- pinned base tool versions have been reviewed deliberately;
- first-use installation, cached/offline reuse, update failure fallback, exact
  version selection, and simultaneous sessions pass against the release image;
- `make verify` passes locally;
- the GitHub `Verify` workflow passes for the release commit on `main`;
- the public `main` image contains a Linux ARM64 manifest built from the
  intended image inputs; and
- the release notes describe the security boundary, known limitations, and
  any incompatible Profile schema change.

## Release procedure

1. Choose the next semantic version.
2. Update `package.version` in `Cargo.toml`; let Cargo update the root package
   entry in `Cargo.lock`.
3. Deliberately update pinned guest-tool versions in
   `images/rust-node/Containerfile` when the release requires them.
4. Update user-facing Profile examples to the exact release image when that
   image becomes the supported default.
5. Run `make format` and `make verify`.
6. Commit the reviewed release changes on `main`, push them, and wait for the
   GitHub `Verify` workflow to pass.
7. Confirm that the moving `main` image represents the intended release image
   inputs.
8. Create and push an annotated `vX.Y.Z` Git tag.
9. Confirm that the release-tag workflow published `X.Y.Z` and `X.Y`, and that
   the `main` build's existing `sha-<full-commit>` image remains unchanged.
   Verify the exact release image through Apple `container` before distributing
   that CLI release. Its `init` default already points at the matching exact
   version.
10. Test the documented `cargo install --locked --git ... --tag vX.Y.Z`
    command from a clean temporary Cargo root.
11. Run `cloister init`, `cloister check`, and one Codex and Claude smoke test
    against the exact release image.
12. Create a GitHub Release from the verified tag with reviewed notes and no
    binary attachments for 0.4.x.
13. Update `Formula/cloister.rb` in `yi427/homebrew-tap` to the immutable release
    archive and SHA-256, then audit, install from source, and test the Formula
    before publishing it.

Example for the `0.4.1` patch release, after all gates pass:

```sh
git tag -a v0.4.1 -m "Release Cloister 0.4.1"
git push origin v0.4.1
```

The publish workflow rejects a release tag whose version does not exactly
match the root `cloister` package version in `Cargo.toml`.

## User Profile upgrade path

Installing a newer CLI does not silently rewrite an existing Profile. Users
can inspect and apply the narrowly scoped official-image update:

```sh
cloister profile upgrade --dry-run
cloister profile upgrade
cloister check
```

The command accepts only an older official exact-release image in a current
schema Profile. `--dry-run` performs no image pull or filesystem mutation. The
apply path checks the target image, asks before pulling when it is absent,
verifies that it has a Linux ARM64 manifest, then asks separately before it:

1. creates an owner-only backup beside the Profile; and
2. atomically replaces only the `image` value while preserving comments,
   unrelated fields, and the original file permissions.

Existing backups are never overwritten. Immutable testing images, custom
images, future releases, moving official tags, symlinks, and incompatible
Profile schemas require manual review instead of automatic conversion.

## 0.4.1 checklist

This checklist is a preparation template, not a live publication-status
tracker. The [GitHub release](https://github.com/yi427/Cloister/releases/tag/v0.4.1)
records completed checks and coverage limits. Copy and update the template
for a future release, and keep its results separate from development testing.

- [ ] MIT OR Apache-2.0 licensing files and Cargo metadata are present.
- [ ] `Cargo.toml`, `Cargo.lock`, `cloister --version`, the example Profile, and
  the default `init` image all resolve to `0.4.1`.
- [ ] Local `make verify` passes.
- [ ] The release commit is pushed and GitHub `Verify` passes.
- [ ] `main` and `origin/main` identify the same release commit.
- [ ] Annotated tag `v0.4.1` is created from that commit and pushed once.
- [ ] Image tags `0.4.1`, `0.4`, and `sha-<release-commit>` exist.
- [ ] Anonymous GHCR access returns a Linux ARM64 manifest for `0.4.1`.
- [ ] Source installation from `v0.4.1` succeeds with `--locked`.
- [ ] `init`, `check`, Codex, Claude, Host Exec, cancellation, and audit smoke
  tests pass against `ghcr.io/yi427/cloister:0.4.1`.
- [ ] A GitHub Release is created with source archives and no binary assets.
- [ ] The official Homebrew Formula points at the exact release archive and
  passes audit, source installation, and `brew test`.

## Local development image

`make image` builds `cloister:dev`. This local tag is intentionally separate
from GHCR release tags and must not appear in Profiles intended for end users.
