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

The `cloister init` command derives its default release image from the compiled
CLI version:

```text
ghcr.io/yi427/cloister:<CARGO_PKG_VERSION>
```

For example, Cloister CLI `0.1.0` selects
`ghcr.io/yi427/cloister:0.1.0`.

## CLI distribution boundary

The 0.1.x CLI is installed from an exact Git tag and compiled locally with
Cargo:

```sh
cargo install --locked \
  --git https://github.com/yi427/Cloister.git \
  --tag v0.1.0 \
  cloister
```

The first release does not attach prebuilt binaries and does not publish a
Homebrew formula. Those are separate future distribution channels. A GitHub
Release may still provide reviewed release notes and GitHub's source archives;
the annotated Git tag remains the source of truth.

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

The `ghcr.io/yi427/cloister` package is public. Every release must verify
anonymous manifest access before the install command is documented as ready.
GitHub package visibility cannot later be changed back to private.

## Release gates

Before creating a release tag, all of the following must be true:

- the working tree is clean and local `main` matches `origin/main`;
- `Cargo.toml` and the root package entry in `Cargo.lock` contain the intended
  version;
- `examples/profile.toml` selects the matching exact `X.Y.Z` image;
- pinned guest tool versions have been reviewed deliberately;
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
9. Confirm that GitHub Actions published `X.Y.Z`, `X.Y`, and the immutable
   `sha-<full-commit>` image tags, then verify the exact image through Apple
   `container` before distributing that CLI release. Its `init` default already
   points at the matching exact version.
10. Test the documented `cargo install --locked --git ... --tag vX.Y.Z`
    command from a clean temporary Cargo root.
11. Run `cloister init`, `cloister check`, and one Codex and Claude smoke test
    against the exact release image.
12. Create a GitHub Release from the verified tag with reviewed notes and no
    binary attachments for 0.1.x.

Example for version `0.1.0`:

```sh
git tag -a v0.1.0 -m "Release Cloister 0.1.0"
git push origin v0.1.0
```

The publish workflow rejects a release tag whose version does not exactly
match the root `cloister` package version in `Cargo.toml`.

## 0.1.0 checklist

- [ ] MIT OR Apache-2.0 licensing files and Cargo metadata are present.
- [ ] `Cargo.toml`, `Cargo.lock`, `cloister --version`, the example Profile, and
  the default `init` image all resolve to `0.1.0`.
- [ ] Local `make verify` passes.
- [ ] The release commit is pushed and GitHub `Verify` passes.
- [ ] `main` and `origin/main` identify the same release commit.
- [ ] Annotated tag `v0.1.0` is created from that commit and pushed once.
- [ ] Image tags `0.1.0`, `0.1`, and `sha-<release-commit>` exist.
- [ ] Anonymous GHCR access returns a Linux ARM64 manifest for `0.1.0`.
- [ ] Source installation from `v0.1.0` succeeds with `--locked`.
- [ ] `init`, `check`, Codex, Claude, Host Exec, cancellation, and audit smoke
  tests pass against `ghcr.io/yi427/cloister:0.1.0`.
- [ ] A GitHub Release is created with source archives and no binary assets.

## Local development image

`make image` builds `cloister:dev`. This local tag is intentionally separate
from GHCR release tags and must not appear in Profiles intended for end users.
