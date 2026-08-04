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
- expose an authenticated host shell escape hatch to Codex by default;
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

Build the current local development image:

```sh
make image
```

The `main` branch also publishes a Linux ARM64 development image to GitHub
Container Registry as `ghcr.io/yi427/cloister:main` and an immutable
`sha-<commit>` tag. A pushed `vX.Y.Z` Git tag publishes `X.Y.Z` and `X.Y` tags;
the workflow deliberately does not publish a floating `latest` tag. GitHub
packages are private on first publication, so a maintainer must explicitly make
the package public before unauthenticated users can pull it.
The complete tag and release policy is documented in
[`docs/releasing.md`](docs/releasing.md).

This produces `cloister:dev` with Node.js, Rust, Git, Codex CLI, and
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
`cloister:dev`, and keeps Codex state in
`~/.local/share/cloister/agents/codex`. Cloister creates that directory with
owner-only permissions and mounts it as `CODEX_HOME`; it never mounts the
host's existing `~/.codex`.

The command also starts an authenticated MCP bridge on macOS loopback and
injects it into Codex as `cloister_host`. Its only tool, `host.exec`, runs an
arbitrary command as the macOS user running Cloister. Codex is configured to
prompt before each call. The bearer token exists only for the process
lifecycle, is forwarded by environment-variable name, and is never printed,
persisted in `config.toml`, or mounted as a file.

The prompt is a Codex MCP interaction policy, not a guest-process security
boundary. The token is available to the Codex process and may be inherited by
processes it starts. A guest process that obtains the token can call the bridge
directly without a Codex approval prompt. Enabling the bridge by default
therefore deliberately grants the guest a path to the macOS user's authority.

Apple `container` must have a localhost DNS domain that forwards the guest name
to macOS loopback. Create it once with:

```sh
sudo container system dns create \
  host.container.internal \
  --localhost 203.0.113.113
```

Confirm it with `container system dns list`. Apple documents that creating a
localhost domain disables Private Relay and that its packet-filter rule is
removed on restart. The MCP server is marked required, so Codex fails visibly
instead of silently starting without `host.exec` when the bridge is not
reachable.

Disable this high-privilege capability for one invocation with:

```sh
cargo run -- codex --no-host-bridge
```

If port `17834` is already in use, select another loopback port with
`--host-bridge-port`.

Inspect this high-level launch without starting a container:

```sh
cargo run -- codex --dry-run
```

Pass arguments to Codex after `--`:

```sh
cargo run -- codex -- --version
```

Profile selection is explicit `--profile`, otherwise
`~/.config/cloister/profile.toml` is required. Initialize it interactively with:

```sh
cargo run -- init
```

`init` asks for the Profile name, exact image reference, guest CPU and memory
limits, and whether agent credentials, settings, and session history should
persist across projects. The policy applies to every supported agent while each
agent keeps a separate Cloister-managed state directory. Its release-image
default is derived from the CLI version, for example
`ghcr.io/yi427/cloister:0.1.0`. A source checkout can explicitly select the
locally built `cloister:dev` image instead.

The command refuses to overwrite any existing file, directory, or symbolic link
at the target Profile path. It writes a newly confirmed Profile atomically with
owner-only permissions. If Apple `container` is missing, `init` prints the
official installation location and can create only the Profile when explicitly
confirmed. If the runtime is stopped, the image is absent, or the host DNS name
is missing, each change is shown and confirmed separately. DNS setup uses
`sudo`, warns that Apple's localhost forwarding disables Private Relay, and is
declined by default. A final readiness report uses the same checks as
`cloister check`, so incomplete or declined setup exits unsuccessfully without
hiding what remains.

Select another new Profile path with:

```sh
cargo run -- init --profile /path/to/profile.toml
```

Check that Cloister is ready before launching Codex:

```sh
cargo run -- check
```

The separate `check` command is read-only. It validates the selected Profile,
confirms that the Apple `container` service is running, verifies that the
Profile's exact image reference has a compatible Linux ARM64 variant, and
checks that `host.container.internal` is configured. It does not start the
runtime, pull or build an image, create the DNS mapping, or change the Profile.
Every applicable check is reported, and the command exits unsuccessfully if any
check fails.

Select a non-default Profile explicitly when needed:

```sh
cargo run -- check --profile examples/profile.toml
```

`cloister profile check <path>` remains the narrower static Profile validation
command; it does not inspect the host runtime, image store, or DNS setup.

`XDG_CONFIG_HOME` and `XDG_DATA_HOME` are respected. Profile V4 uses
`[agent] state = "shared"` or `"isolated"`. The latter selects temporary
per-container state instead of cross-project persistent state. Shared state can
contain authentication tokens, configuration, history, and skills, so it must
be treated as a secret. Profile V3 and its former `[codex]` table are rejected;
there is no compatibility or automatic migration layer during development.

Workspace selection is intentionally not part of the Profile. The Codex command
mounts the current directory at `/workspace` by default. Select another project
for one invocation with:

```sh
cargo run -- codex --workspace /path/to/project
```

Check the example profile through the CLI:

```sh
cargo run -- profile check examples/profile.toml
```

The Host Bridge can also be started manually for diagnostics with an unused
token path:

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
using it gives the guest an escape hatch from the container boundary.

## Project layout

```text
src/
├── lib.rs                  Library entry point
├── main.rs                 Terminal application entry point
├── error.rs                Centralized error messages
├── agent/                  Agent-specific state and command adapters
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
