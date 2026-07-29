# Cloister

Cloister is a terminal-first, privacy-oriented development environment for AI
coding agents. It will use Apple's `container` CLI to run Codex, Claude Code,
and development toolchains inside lightweight Linux virtual machines on Apple
silicon.

The project now has a first natural Codex workflow. The Rust binary can launch
Codex in the current project with persistent Cloister-managed state, load
profiles, produce inspectable runtime plans, run commands through Apple
`container`, and serve an authenticated host-command bridge.

## MVP boundary

The first useful version will:

- read a versioned TOML profile;
- start an ARM64 Linux environment through Apple `container`;
- apply CPU, memory, locale, timezone, and guest-user settings;
- expose the selected project through an explicit live bind mount;
- keep Cloister-managed agent state separate from host credentials;
- optionally expose an authenticated host shell escape hatch;
- make network and writable-mount choices visible before launch;
- stop and remove environments without deleting the host project.

It will not initially provide a general-purpose container runtime, a GUI, or
transparent bidirectional file synchronization.

## Development baseline

- Host: Apple silicon Mac
- Runtime: Apple `container` 1.2 or newer
- Language: Rust 1.97.1, edition 2024
- Guest direction: Debian-family ARM64 image
- Agent runtime direction: Node.js LTS plus pinned Codex and Claude Code builds

The current profile shape is illustrated in
[`examples/codex.toml`](examples/codex.toml). Architectural and security
decisions are recorded in
[`docs/adr/0001-development-environment.md`](docs/adr/0001-development-environment.md).

Build the current local development image:

```sh
make image
```

This produces `cloister/rust-node:dev` with Node.js, Rust, Git, Codex CLI, and
Claude Code installed. The tool versions are pinned in
[`images/rust-node/Containerfile`](images/rust-node/Containerfile). The image
uses a non-root `cloister` user and keeps its temporary home and CLI state under
the guest `/tmp` tmpfs. It does not contain or mount host credentials.

Run Codex in the current project:

```sh
cd /path/to/project
cloister codex
```

When running from this repository during development, use:

```sh
cargo run -- codex
```

The command maps the current directory to `/workspace`, reuses
`cloister/rust-node:dev`, and keeps Codex state in
`~/.local/share/cloister/agents/codex`. Cloister creates that directory with
owner-only permissions and mounts it as `CODEX_HOME`; it never mounts the
host's existing `~/.codex`.

Inspect this high-level launch without starting a container:

```sh
cargo run -- codex --dry-run
```

Pass arguments to Codex after `--`:

```sh
cargo run -- codex -- --version
```

Profile selection is explicit `--profile`, otherwise
`~/.config/cloister/profile.toml` is required. To create the default Profile
from the documented example:

```sh
mkdir -p ~/.config/cloister
cp examples/codex.toml ~/.config/cloister/profile.toml
```

`XDG_CONFIG_HOME` and `XDG_DATA_HOME` are respected. A Codex Profile can set
`state = "isolated"` for temporary per-container state instead of the default
cross-project shared state. Shared state can contain authentication tokens,
configuration, history, and skills, so it must be treated as a secret.

Workspace selection is intentionally not part of the Profile. The Codex command
mounts the current directory at `/workspace` by default. Select another project
for one invocation with:

```sh
cargo run -- codex --workspace /path/to/project
```

Check the example profile through the CLI:

```sh
cargo run -- profile check examples/codex.toml
```

The current Host Bridge prototype exposes one MCP tool, `host.exec`. Start it
with an unused token path:

```sh
cargo run -- host serve \
  --listen 127.0.0.1:17834 \
  --token-file /private/tmp/cloister-bridge.token
```

Exercise it from another process:

```sh
cargo run -- host exec \
  --endpoint http://127.0.0.1:17834/mcp \
  --token-file /private/tmp/cloister-bridge.token \
  'xcodebuild -version'
```

The token is generated with owner-only permissions and is never printed.
The bridge refuses non-loopback listeners. `host.exec` deliberately allows
arbitrary commands with the permissions of the macOS user running Cloister;
using it gives the AI an escape hatch from the container boundary.
Container-to-host transport wiring remains a later slice.

## Project layout

```text
src/
├── lib.rs                  Library entry point
├── main.rs                 Terminal application entry point
├── error.rs                Centralized error messages
├── cli/                    clap commands, including the natural Codex entry point
├── host_bridge/            Authenticated host shell MCP bridge
├── preflight/              Host path resolution and checks
├── runtime/                Inspectable plans and container arguments
└── profile/
    ├── mod.rs              Profile module boundary
    ├── loader.rs           File reading, parsing, and validation pipeline
    ├── model.rs            Versioned profile data model
    ├── parser.rs           Side-effect-free parsing
    └── validation.rs       Fail-closed semantic validation
tests/           Cross-module and CLI integration tests
tests/fixtures/  Deterministic test inputs and expected outputs
examples/        User-facing configuration examples
docs/adr/        Architecture decision records
```

## Development workflow

Enable the repository-managed Git hooks once after cloning:

```sh
make install-hooks
```

Format the working tree:

```sh
make format
```

Run the same verification used before each commit:

```sh
make verify
```

The pre-commit hook verifies the exact staged snapshot in a temporary directory.
It does not modify files or stage formatting changes automatically. A commit is
rejected if formatting, compilation, tests, or Clippy fail.
