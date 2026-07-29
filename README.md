# Cloister

Cloister is a terminal-first, privacy-oriented development environment for AI
coding agents. It will use Apple's `container` CLI to run Codex, Claude Code,
and development toolchains inside lightweight Linux virtual machines on Apple
silicon.

The project is currently at the first executable-runtime stage. The Rust binary
can load profiles, produce inspectable runtime plans, run commands through Apple
`container`, and serve an authenticated host-command bridge.

## MVP boundary

The first useful version will:

- read a versioned TOML profile;
- start an ARM64 Linux environment through Apple `container`;
- apply CPU, memory, locale, timezone, user, and workspace settings;
- support a live bind-mounted workspace and an isolated copy workspace;
- keep each environment's agent state separate from host credentials;
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
[`examples/profile.toml`](examples/profile.toml). Architectural and security
decisions are recorded in
[`docs/adr/0001-development-environment.md`](docs/adr/0001-development-environment.md).

Check the example profile through the CLI:

```sh
cargo run -- profile check examples/profile.toml
```

Inspect the Apple container command plan without starting a virtual machine:

```sh
cargo run -- run --profile examples/profile.toml --dry-run -- /bin/sh
```

Run a command in a temporary environment:

```sh
cargo run -- run --profile examples/smoke.toml -- /bin/sh
```

Verify the project mount non-interactively:

```sh
cargo run -- run --profile examples/smoke.toml -- \
  /bin/sh -lc 'pwd && test -f smoke.toml && echo "workspace ready"'
```

Relative `workspace.host` values are resolved from the directory containing the
profile file, not from the shell's current working directory. The smoke profile
therefore mounts the `examples/` directory that contains it. It intentionally
uses a public Debian image and does not contain Codex or Claude Code.

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
├── cli/                    clap command definitions and dispatch
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
