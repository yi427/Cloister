# Cloister

[![Verify](https://github.com/yi427/Cloister/actions/workflows/verify.yml/badge.svg)](https://github.com/yi427/Cloister/actions/workflows/verify.yml)
[![Publish image](https://github.com/yi427/Cloister/actions/workflows/publish-image.yml/badge.svg)](https://github.com/yi427/Cloister/actions/workflows/publish-image.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Cloister is a terminal-first, privacy-oriented development environment for AI
coding agents on Apple silicon Macs. It runs Codex and Claude Code inside a
lightweight Linux virtual machine managed by Apple's [`container`](https://github.com/apple/container)
CLI, while keeping every host mount and host-command capability explicit.

Cloister is pre-1.0 software. Profile compatibility and security-boundary
changes may still require deliberate configuration updates between releases.

## Why Cloister

AI coding agents are useful precisely because they can inspect files, run
tools, and modify projects. Cloister makes those capabilities reviewable:

- the selected project is the only workspace mounted by default;
- agent credentials and settings live in Cloister-managed state instead of the
  host's existing `~/.codex` or `~/.claude` directories;
- CPU, memory, locale, timezone, user, image, network, and proxy choices come
  from a versioned Profile;
- optional host execution is enforced by an immutable executable allowlist;
- runtime plans, readiness checks, and audit records expose the active
  boundaries without printing secrets; and
- Codex and Claude Code use the same canonical Host Exec skill and policy.

## Security model

Cloister reduces ambient host access; it is not a complete sandbox for every
capability it exposes.

> [!WARNING]
> The workspace is a live read-write bind mount. An agent can modify or delete
> files in the selected project.

> [!WARNING]
> Host Exec is an explicit escape hatch from the guest boundary. An allowed
> command runs as the macOS user who started Cloister. Interpreters and build
> tools such as `python`, `cargo`, and `xcodebuild` may provide broad host code
> execution even when only their executable names are allowlisted.

The following claims are intentionally narrow:

- host credential directories and privileged sockets are never mounted
  silently;
- Profile validation and Host Exec allowlist checks fail closed;
- arguments are passed directly as an argument vector without shell parsing;
- the default Apple container network may reach the internet and is not an
  egress firewall;
- an agent approval prompt is an interaction policy, not a security boundary;
- a guest process that obtains the ephemeral bridge token can call enabled
  bridge tools directly, although every execution still passes server-side
  Profile validation; and
- Host Exec audit logs are observational, not tamper-proof, because an allowed
  host command may have enough macOS permission to alter them.

Read the accepted architecture decisions under [`docs/adr`](docs/adr) before
changing these boundaries.

## Requirements

- Apple silicon Mac
- Apple `container` 1.2 or newer
- Rust 1.97.1 or newer with Cargo for direct source installation
- a Linux ARM64 Cloister image matching the CLI release

Install Apple's signed package from the
[`container` releases page](https://github.com/apple/container/releases).

## Installation

Cloister 0.1.x is available from the official Homebrew tap. Homebrew installs
the Apple `container` dependency and builds Cloister from the immutable release
source. It does not start the runtime, create a Profile, or pull the guest
image.

```sh
brew install yi427/tap/cloister
```

Alternatively, install directly from the exact Git tag with Cargo:

```sh
cargo install --locked \
  --git https://github.com/yi427/Cloister.git \
  --tag v0.1.0 \
  cloister
```

The CLI tag and guest image are one release pair:

```text
CLI:   v0.1.0
Image: ghcr.io/yi427/cloister:0.1.0
```

Cloister refuses to start an agent when an official release image does not
exactly match the CLI version. After upgrading the CLI, preview and apply the
corresponding Profile update explicitly:

```sh
cloister profile upgrade --dry-run
cloister profile upgrade
cloister check
```

The upgrade command verifies the replacement Linux ARM64 image before asking
to back up and atomically update the Profile. It never rewrites a Profile or
pulls an image during `--dry-run`.

## Quick start

Create the default Profile interactively:

```sh
cloister init
```

`init` can start Apple `container`, pull the exact release image, and create
the required localhost DNS mapping. Every mutation is shown and confirmed
separately. Existing files, directories, and symbolic links at the Profile
path are never overwritten.

Verify the complete environment without changing it:

```sh
cloister check
```

Then launch an agent in a project:

```sh
cd /path/to/project
cloister codex
# or
cloister claude
```

The current directory is mounted at `/workspace`. Select another directory for
one invocation with `--workspace`.

## Architecture

```text
macOS host
├── Cloister CLI
│   ├── Profile loading and readiness checks
│   ├── Apple container command construction
│   └── authenticated loopback Host MCP bridge
│       └── Profile-allowed macOS commands
└── Apple container VM
    ├── /workspace                 live read-write project mount
    ├── /cloister/agents/<agent>   optional Cloister-managed shared state
    ├── Codex or Claude Code
    └── canonical host-exec skill
```

The container is ephemeral and removed after the agent exits. The workspace
and optional shared agent state remain on the host.

## Profiles

Cloister uses a versioned TOML Profile as its public configuration contract.
The default path is:

```text
${XDG_CONFIG_HOME:-~/.config}/cloister/profile.toml
```

An abbreviated Profile V6 looks like this:

```toml
schema_version = 6
name = "default"

[image]
reference = "ghcr.io/yi427/cloister:0.1.0"
architecture = "arm64"

[guest]
cpus = 4
memory = "8G"
user = "cloister"
locale = "en_US.UTF-8"
timezone = "America/New_York"

[network]
mode = "default"
proxy = "disabled" # or "inherit"

[agent]
state = "shared" # or "isolated"

[host.exec]
enabled = true

[host.exec.environment]
mode = "inherit-all"

[[host.exec.allow]]
name = "xcodebuild"
executable = "/usr/bin/xcodebuild"
description = "Build and test Xcode projects"
arguments = "any"
```

The complete release-aligned example is
[`examples/profile.toml`](examples/profile.toml). Earlier Profile versions are
rejected. Cloister does not silently migrate incompatible Profiles during the
pre-1.0 development period.

### Agent state

`state = "shared"` preserves the selected agent's credentials, settings,
history, and sessions across projects. Codex and Claude use separate
Cloister-managed directories:

```text
${XDG_DATA_HOME:-~/.local/share}/cloister/agents/codex
${XDG_DATA_HOME:-~/.local/share}/cloister/agents/claude
```

These directories are owner-only and should be treated as secrets. Cloister
never substitutes the host's pre-existing agent configuration directories.
`state = "isolated"` keeps agent state inside the ephemeral container instead.

### Network and proxy policy

`network.mode = "default"` uses Apple container's default networking. It does
not mean internet access is blocked.

`proxy = "inherit"` selects the first supported host HTTP proxy variable,
rewrites a loopback host to `host.container.internal`, and extends `NO_PROXY`
so Host MCP traffic remains direct. Proxy values may contain credentials and
are visible to guest processes. Cloister does not store or print those values.
Use `proxy = "disabled"` when inheritance is not required.

The complete contract is documented in
[`ADR 0005`](docs/adr/0005-inherited-guest-proxy.md).

## Host Exec

When `[host.exec] enabled = true`, Cloister starts an authenticated MCP server
on macOS loopback and injects it into the selected agent as `cloister_host`.
The server exposes four tools:

| Tool | Purpose |
| --- | --- |
| `host.list_commands` | Discover the fixed Host working directory, immutable Profile allowlist, and non-secret environment metadata. |
| `host.exec` | Start one allowed executable with a literal argument vector. |
| `host.exec_status` | Read state and incremental output, optionally waiting for a change. |
| `host.exec_cancel` | Cancel an execution and its process group. |

The first policy version allows arbitrary arguments for an allowed executable.
This broad permission is represented explicitly as `arguments = "any"`.

Long-running work has no global timeout. `host.exec` returns an execution ID
after a 100 ms inline window. Callers can ask `host.exec_status` to wait up to
30 seconds for retained output or a terminal state instead of repeatedly
polling unchanged state. The bridge limits accidental resource growth to eight
concurrent executions, 32 concurrent status waits, 1 MiB of retained output
per execution, and 128 in-memory execution records. Jobs and captured output
do not survive a bridge restart.

The canonical model instructions live in
[`skills/host-exec/SKILL.md`](skills/host-exec/SKILL.md). Codex receives a
transient MCP configuration that marks the server as required. Claude loads the
server eagerly, but Claude Code does not expose an equivalent fail-closed
`required` setting; connection failure is reported by Claude instead.

Disable the bridge for one invocation when host access is unnecessary:

```sh
cloister codex --no-host-bridge
cloister claude --no-host-bridge
```

The default bridge port is `17834`. Use `--host-bridge-port <PORT>` when another
Cloister process already owns that port.

### DNS requirement

The guest resolves the loopback bridge through `host.container.internal`.
`cloister init` can create this mapping, or it can be created explicitly:

```sh
sudo container system dns create \
  host.container.internal \
  --localhost 203.0.113.113
```

Confirm it with `container system dns list`. Apple documents that creating a
localhost domain disables Private Relay and that its packet-filter rule is
removed on restart. `cloister check` reports when the mapping is absent.

### Audit log

Host Exec writes versioned JSONL lifecycle metadata to:

```text
${XDG_STATE_HOME:-~/.local/state}/cloister/audit/host-exec.jsonl
```

The active file and one rotated segment are each limited to 10 MiB. Directories
use mode `0700`; log and lock files use `0600`. Rotation is coordinated across
bridge processes. Unsafe ownership, permissions, links, or oversized existing
segments are rejected instead of repaired silently.

Raw arguments, stdout, stderr, environment values, bearer tokens, and agent
credentials are not persisted. Audit events retain command identity, declared
and resolved paths, timestamps, outcome, duration, byte counts, and truncation
metadata.

See [`ADR 0004`](docs/adr/0004-profile-governed-host-execution.md) for the full
policy and audit design.

## Command reference

| Command | Description |
| --- | --- |
| `cloister init` | Interactively create a new Profile and prepare missing Apple container components. |
| `cloister check` | Read-only readiness check for Profile, proxy policy, host paths, runtime, image, and DNS. |
| `cloister codex` | Run Codex in the selected project. |
| `cloister claude` | Run Claude Code in the selected project. |
| `cloister profile check <PATH>` | Parse and statically validate one Profile without inspecting runtime state. |
| `cloister profile upgrade [--profile <PATH>] [--dry-run]` | Preview or apply an older official release image update for the current CLI. |
| `cloister host serve` | Start the authenticated Host MCP bridge manually for diagnostics. |
| `cloister host exec` | Exercise one allowed command through a running bridge. |

Common agent options:

```text
--profile <PATH>          select a non-default Profile
--workspace <DIRECTORY>  mount another project at /workspace
--dry-run                print the runtime plan without starting an agent
--no-host-bridge         reduce authority for one invocation
--host-bridge-port <N>   select another loopback bridge port
-- <ARGUMENTS>           pass remaining arguments directly to the agent
```

Run `cloister <COMMAND> --help` for the complete CLI-generated reference.

## Images and versioning

Release Profiles should use an exact image tag matching the CLI version.
Cloister does not publish a floating `latest` tag.

| Tag | Meaning | Mutable |
| --- | --- | --- |
| `main` | Latest successful development image from `main`. | Yes |
| `sha-<commit>` | Image built from one exact commit. | No |
| `X.Y.Z` | Exact release image paired with CLI tag `vX.Y.Z`. | No |
| `X.Y` | Latest patch image in a minor release line. | Yes |

Cloister classifies an official image before starting an agent:

- `ghcr.io/yi427/cloister:X.Y.Z` must exactly match the CLI version;
- `ghcr.io/yi427/cloister:sha-<full-commit>` is allowed for explicit,
  immutable testing and produces a warning;
- official moving tags such as `main`, `latest`, and `X.Y` are rejected; and
- local or third-party images remain available for development but produce a
  warning because Cloister cannot verify their release compatibility.

`cloister check` reports this classification without changing runtime or
Profile state. Both normal execution and `--dry-run` agent plans fail before
workspace, bridge, or container side effects when an official release pair is
incompatible.

The Linux ARM64 image contains pinned Node.js, Rust, Codex, and Claude Code
versions, plus Git, build tools, `ripgrep`, and `bubblewrap`. It runs as the
non-root `cloister` user and contains the canonical Host Exec skill and project
licenses. Exact versions are maintained in
[`images/rust-node/Containerfile`](images/rust-node/Containerfile).

See [`docs/releasing.md`](docs/releasing.md) for the release gates and tag
policy.

## Limitations

Cloister 0.1.x intentionally does not provide:

- Intel Mac, Linux-host, Docker, or general-purpose container-runtime support;
- a GUI, prebuilt standalone CLI binary, or transparent bidirectional file
  synchronization;
- automatic migration of incompatible Profile versions;
- argument-level Host Exec restrictions beyond `arguments = "any"`;
- host-level CPU, memory, filesystem, or network containment for Host Exec;
- restoration of running Host Exec jobs after Cloister exits; or
- automatic selection of an unused Host MCP port.

## Development

Clone the repository and install its staged-snapshot Git hook:

```sh
git clone https://github.com/yi427/Cloister.git
cd Cloister
make install-hooks
```

Build the local development image:

```sh
make image
```

This creates `cloister:dev`. A development Profile must select that tag
explicitly and Cloister reports it as an unverified custom-image warning;
user-facing release Profiles use exact GHCR versions.

Run the CLI from the checkout:

```sh
cargo run -- check
cargo run -- codex --dry-run
cargo run -- claude --dry-run
```

Format and verify all targets:

```sh
make format
make verify
```

`make verify` checks formatting, image inputs, compilation, all tests, and
Clippy with warnings denied. The pre-commit hook runs the same command against
the exact staged snapshot without modifying or staging the working tree.
GitHub's `Verify` workflow applies the same gate to pull requests and pushes to
`main`.

## Project layout

```text
src/          Rust library and CLI implementation
tests/        public behavior, cross-module, CLI, image, and release contracts
skills/       canonical agent skill sources
images/       Apple-container-compatible OCI image inputs
examples/     release-aligned user configuration
docs/adr/     accepted architecture and security decisions
```

## Contributing

Keep changes small, explicit, and reviewable. Add focused module tests for
Profile parsing or command construction, and public-behavior tests under
`tests/`. Do not weaken documented security claims or introduce hidden host
mounts, credentials, sockets, network assumptions, or policy defaults. Run
`make verify` before opening a pull request.

## License

Cloister is licensed under either the
[Apache License, Version 2.0](LICENSE-APACHE) or the [MIT License](LICENSE-MIT),
at your option.
