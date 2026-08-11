# ADR 0007: CLI and image release compatibility

- Status: Accepted, implemented
- Date: 2026-08-11
- Updates: ADR 0001 and ADR 0003

## Context

Cloister releases the macOS CLI and Linux ARM64 Guest image as one tested
pair. `cloister init` writes the compiled CLI version into the default image
reference, but an existing Profile retains that exact reference when Homebrew
later upgrades the CLI. Before this decision, `cloister check` verified only
that the selected image existed and contained a Linux ARM64 variant. Codex and
Claude launches did not reject an old official image paired with a newer CLI.

That gap is significant because the Guest image contains the canonical Agent
Skill and pinned Agent versions while the CLI owns container planning and the
Host Bridge. A mixed release may appear to work while using instructions or a
tool contract that was not accepted as one release.

Cloister also needs an explicit development path. Immutable `sha-<commit>`
images and local `cloister:dev` images are useful before a release exists, but
they must not be described as release-compatible.

## Decision

### Classify image references locally

Cloister classifies the Profile image reference against the running CLI's
`CARGO_PKG_VERSION` without querying a release service:

- `ghcr.io/yi427/cloister:X.Y.Z` is a paired release only when `X.Y.Z` exactly
  matches the CLI version;
- a different official `X.Y.Z` is an error;
- `ghcr.io/yi427/cloister:sha-<full-commit>` is an immutable testing image and
  is allowed with a warning;
- official `main`, `latest`, and `X.Y` tags are rejected because they can move
  without changing the Profile; and
- local or third-party image references remain explicit custom images and are
  allowed with a warning that compatibility cannot be verified.

An official tag not covered by these forms is rejected rather than inferred to
be a release or testing channel. A testing or custom image is allowed, not
certified: Cloister makes no claim that its Skill, Agent versions, or other
contents match the CLI.

### Enforce the decision before Agent side effects

`cloister check` reports image compatibility independently from runtime image
availability. A paired release is `PASS`, a testing or custom image is `WARN`,
and an official mismatch or moving tag is `FAIL`. Warnings do not make the
read-only check fail.

Natural Codex and Claude entry points apply the same rule immediately after
loading the Profile. They reject an invalid official combination before
creating Agent state, starting the Host Bridge, or calling Apple `container`.
`--dry-run` does not bypass the rule. Allowed testing and custom images emit a
visible warning before their runtime plan or launch.

Manual `cloister host` commands do not launch a Guest image and therefore do
not enforce CLI-image pairing.

### Upgrade an existing current-schema Profile explicitly

`cloister profile upgrade` changes only an older official exact release image
to the exact release image paired with the running CLI. It does not upgrade the
CLI itself; Homebrew or the exact Git tag remains the CLI installation
boundary.

The upgrade flow:

1. loads and validates a regular, non-symbolic-link current-schema Profile;
2. reports the Profile path, unchanged schema, current and target image, and
   backup path;
3. supports `--dry-run` without inspecting or pulling an image and without
   changing files;
4. confirms and pulls the exact target ARM64 image when it is absent;
5. verifies that the target contains a Linux ARM64 variant;
6. separately confirms the Profile change;
7. creates a new owner-only backup without overwriting an existing backup; and
8. replaces only the spanned TOML image value through an atomic same-directory
   write, preserving comments, unrelated settings, and the Profile mode.

The Profile remains unchanged if image preparation or verification fails or if
the user declines the file update. A pulled but unused image may remain. Old
images are never deleted automatically.

The command refuses testing images, custom images, moving tags, newer official
images, symbolic-link Profiles, and incompatible Profile schemas. Schema
migration requires a separate versioned decision.

## Compatibility and versioning

This decision does not change Profile V6. It defines how the existing image
reference is interpreted by release-aware commands and adds a CLI subcommand.
The Profile schema continues to change only for incompatible configuration
contracts, not for every application release.

Release preparation still updates `Cargo.toml`, `Cargo.lock`, the example
Profile, documentation, Git tag, exact GHCR image, and Homebrew Formula as one
reviewed release. After a package-manager upgrade, users run:

```sh
cloister profile upgrade --dry-run
cloister profile upgrade
cloister check
```

Users who intentionally remain on an old official image must also retain or
reinstall the matching old CLI release.

## Security consequences

- Official release skew fails closed instead of silently using an untested
  Skill and CLI combination.
- Mutable official tags cannot change the Guest behind a persistent Profile.
- Testing and custom images remain explicit authority choices and are never
  promoted to verified release compatibility.
- Upgrade does not follow symbolic links, overwrite a backup, edit unrelated
  Profile fields, delete old images, or mutate state during dry-run.
- No background update check or implicit network request is introduced.

## Deferred work

This decision does not add:

- automatic CLI installation or Homebrew updates;
- automatic Profile schema migration;
- a compatibility range or multi-release support matrix;
- automatic testing or custom image replacement;
- automatic old-image cleanup; or
- a floating release Profile channel.

## Validation

The implementation includes unit and public CLI coverage for exact release
pairing, older and newer official images, immutable full-commit testing tags,
moving and unsupported official tags, local and custom images, read-only check
warnings, pre-side-effect Codex and Claude refusal, source-preserving dry-run
and upgrade, pull and ARM64 verification ordering, owner-only backup creation,
mode preservation, user cancellation, and symbolic-link rejection.
