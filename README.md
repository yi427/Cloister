# Cloister

Cloister is a terminal-first, privacy-oriented development environment for AI
coding agents. It will use Apple's `container` CLI to run Codex, Claude Code,
and development toolchains inside lightweight Linux virtual machines on Apple
silicon.

The project now has natural Codex and Claude Code workflows. The Rust binary
can launch either agent in the current project with separate persistent
Cloister-managed state, load profiles, produce inspectable runtime plans, run
commands through Apple `container`, and serve an authenticated host-command
bridge.

## MVP boundary

The first useful version will:

- read a versioned TOML profile;
- start an ARM64 Linux environment through Apple `container`;
- apply CPU, memory, locale, timezone, and guest-user settings;
- expose the selected project through an explicit live bind mount;
- keep Cloister-managed agent state separate from host credentials;
- expose an authenticated host shell escape hatch to supported agents by
  default;
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

This produces `cloister:dev` with Node.js, Rust, Git, Codex CLI, Claude Code,
and Debian's `bubblewrap` package installed. Providing `bwrap` on `PATH` lets
Codex use its normal Linux sandbox without falling back to its bundled helper
or printing the missing-bubblewrap startup warning. The tool versions are
pinned in [`images/rust-node/Containerfile`](images/rust-node/Containerfile).
The image uses a non-root `cloister` user and keeps its temporary home and CLI
state under the guest `/tmp` tmpfs. It does not contain or mount host
credentials.

Run Codex or Claude Code in the current project:

```sh
cd /path/to/project
cloister codex
cloister claude
```

When running from this repository during development, use:

```sh
cargo run -- codex
cargo run -- claude
```

Both commands map the current directory to `/workspace` and reuse
`cloister:dev`. Shared Codex state lives under
`~/.local/share/cloister/agents/codex` and is mounted as `CODEX_HOME`; shared
Claude state lives under `~/.local/share/cloister/agents/claude` and is mounted
as `CLAUDE_CONFIG_DIR`. Cloister creates only the selected agent's directory
with owner-only permissions. It never mounts the host's existing `~/.codex` or
`~/.claude`.

Profile V6 makes guest proxy inheritance explicit:

```toml
[network]
mode = "default"
proxy = "disabled" # or "inherit"
```

`inherit` selects the first non-empty host variable in this order:
`HTTPS_PROXY`, `https_proxy`, `ALL_PROXY`, `all_proxy`, `HTTP_PROXY`, then
`http_proxy`. The value must be an HTTP or HTTPS URL. A loopback proxy host is
rewritten to `host.container.internal`, the same resolved URL is exposed under
the conventional upper- and lowercase HTTP, HTTPS, and ALL proxy variable
names, and Cloister extends `NO_PROXY`/`no_proxy` so the Host MCP bridge stays
direct. Proxy values are supplied only through the host `container` process
environment while the command line contains `--env NAME`; plans, checks, and
debug output show only the source variable and rewrite status.

This exposes the proxy URL, including any embedded credentials, to processes in
the guest. It does not store that URL in the Profile, and it is a connectivity
setting rather than an egress firewall.

When `[host.exec] enabled = true`, each command also starts an authenticated MCP
bridge on macOS loopback and injects it into the selected agent as
`cloister_host`. `host.list_commands` reports the immutable Profile allowlist,
argument policy, and inherited environment variable names without values.
`host.exec` accepts structured `version`, `command`, and `args` fields, resolves
the executable only from that allowlist, and passes arguments directly without
a shell. It returns an execution ID after a 100 ms inline window. Callers use
`host.exec_status` with an output cursor for long-running commands and
`host.exec_cancel` to terminate an unwanted execution and its process group.
There is no global execution timeout.

Host Exec writes versioned JSONL lifecycle metadata to
`${XDG_STATE_HOME:-~/.local/state}/cloister/audit/host-exec.jsonl`. The
`cloister` and `audit` directories are owner-only (`0700`), log and lock files
are `0600`, and symbolic links, hard-linked files, wrong owners, or broader
permissions are rejected. The active and rotated files are each limited to
10 MiB, for a 20 MiB total log bound; oversized existing segments are rejected
rather than truncated. Rotation and appends are coordinated by an inter-process
file lock, and Bridge shutdown drains the audit writer. Raw
arguments, stdout, stderr, environment values, bearer tokens, and agent
credentials are never persisted. Because Host Exec runs as the same macOS
user, an allowed host command can still modify or delete these observational
logs; they are not a tamper-proof security boundary.

The canonical current-surface instructions for models live in
[`skills/host-exec/SKILL.md`](skills/host-exec/SKILL.md). The Skill requires
policy discovery before execution, literal structured arguments, bounded
status polling, cancellation, accurate approval and result reporting, and no
fallback around a denial. The image contains one read-only canonical source. When the Host Bridge
is enabled, Codex receives a temporary symlink under `$HOME/.agents/skills`,
while Claude receives an image-owned `--add-dir` whose `.claude/skills` entry
points to the same source. Neither path writes to `CODEX_HOME` or
`CLAUDE_CONFIG_DIR`. If shared Claude state already contains
`skills/host-exec`, Cloister refuses to start the bridge instead of overwriting
or silently shadowing either Skill. `--no-host-bridge` exposes neither path and
does not apply that conflict check.

Codex receives transient `--config` values that require the server, enable all
four Host tools, and request per-call approval only for `host.exec`. Claude
receives a transient inline `--mcp-config`; the server is
loaded eagerly, and only `host.exec` carries Claude's
`anthropic/requiresUserInteraction` metadata. The bearer token exists only for
the process lifecycle, is forwarded by environment-variable name, and is never
printed, persisted in an agent configuration file, or mounted as a file.

The prompt is an agent interaction policy, not a guest-process security
boundary. The token is available to the agent process and may be inherited by
processes it starts. A guest process that obtains the token can call the bridge
directly without an agent approval prompt, but every execution is still checked
against the immutable server-side Profile policy. Allowed interpreters and build
tools may themselves provide broad macOS user authority.

Apple `container` must have a localhost DNS domain that forwards the guest name
to macOS loopback. Create it once with:

```sh
sudo container system dns create \
  host.container.internal \
  --localhost 203.0.113.113
```

Confirm it with `container system dns list`. Apple documents that creating a
localhost domain disables Private Relay and that its packet-filter rule is
removed on restart. Codex marks the MCP server as required. Claude's
`alwaysLoad` setting blocks initial tool loading while it attempts the bridge
connection, but Claude Code does not expose the same fail-closed `required`
setting; a connection failure is reported by Claude rather than represented as
an equivalent Cloister guarantee.

Disable this high-privilege capability for one invocation with:

```sh
cargo run -- codex --no-host-bridge
cargo run -- claude --no-host-bridge
```

If port `17834` is already in use, select another loopback port with
`--host-bridge-port`.

Inspect this high-level launch without starting a container:

```sh
cargo run -- codex --dry-run
cargo run -- claude --dry-run
```

Pass arguments to the selected agent after `--`:

```sh
cargo run -- codex -- --version
cargo run -- claude -- --version
```

Profile selection is explicit `--profile`, otherwise
`~/.config/cloister/profile.toml` is required. Initialize it interactively with:

```sh
cargo run -- init
```

`init` asks for the Profile name, exact image reference, guest CPU and memory
limits, whether agent credentials, settings, and session history should persist
across projects, whether a detected supported host HTTP proxy should be
inherited, and an optional comma-separated list of host command names. The
proxy URL itself is never printed or stored; when no supported host proxy is
present, the generated policy is `proxy = "disabled"`.
The policy applies to every supported agent while each agent keeps a separate
Cloister-managed state directory. The Host Exec allowlist is empty by default.
Only names explicitly entered by the user are resolved from absolute directories
in the current `PATH`; `init` invokes neither a shell nor `which`, and shows both
the selected path and its canonical target before confirmation. Each selected
command is stored with `arguments = "any"` in the enabled `inherit-all` policy.
Its release-image default is derived from the CLI version, for example
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

Check that Cloister is ready before launching an agent:

```sh
cargo run -- check
```

The separate `check` command is read-only. It validates the selected Profile,
resolves the explicit guest proxy policy from the current host environment
without printing its value or making an upstream request,
inspects every declared Host Exec path and its canonical target, confirms that
each target is a regular file with an execute permission bit, confirms that the
Apple `container` service is running, verifies that the Profile's exact image
reference has a compatible Linux ARM64 variant, and checks that
`host.container.internal` is configured. When a declared executable is stale,
`check` may show a replacement found in an absolute directory from the current
`PATH`, but it never rewrites the Profile. It does not start the runtime, pull
or build an image, create the DNS mapping, or change the Profile. Every
applicable check is reported, and the command exits unsuccessfully if any check
fails.

Select a non-default Profile explicitly when needed:

```sh
cargo run -- check --profile examples/profile.toml
```

`cloister profile check <path>` remains the narrower static Profile validation
command; it does not inspect the host runtime, image store, or DNS setup.

`XDG_CONFIG_HOME` and `XDG_DATA_HOME` are respected. Profile V6 uses
`[agent] state = "shared"` or `"isolated"`. The latter selects temporary
per-container state instead of cross-project persistent state. Shared state can
contain authentication tokens, configuration, history, and skills, so it must
be treated as a secret. Profile V6 retains the explicit `[host.exec]`
allowlist contract with `inherit-all` environment semantics. It is parsed and
validated before launch and enforced independently by the Host MCP server on
every execution request. Earlier Profile versions are rejected; there is no
compatibility or automatic migration layer during development.

Workspace selection is intentionally not part of the Profile. Both agent
commands mount the current directory at `/workspace` by default. Select another
project for one invocation with:

```sh
cargo run -- codex --workspace /path/to/project
cargo run -- claude --workspace /path/to/project
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
  --token-file /private/tmp/cloister-bridge.token \
  --profile examples/profile.toml
```

Exercise it from another process:

```sh
cargo run -- host exec \
  --endpoint http://127.0.0.1:17834/mcp \
  --token-file /private/tmp/cloister-bridge.token \
  xcodebuild -- -version
```

The token is generated with owner-only permissions and is never printed.
The bridge refuses non-loopback listeners. `host.exec` runs only a command in
the selected Profile allowlist, but that command still has the permissions of
the macOS user running Cloister and remains an explicit escape hatch from the
container boundary.

The active policy design and remaining work are documented in
[`ADR 0004`](docs/adr/0004-profile-governed-host-execution.md). It defines a
Profile-governed executable allowlist, command discovery, structured argv,
controlled environment inheritance, asynchronous execution status and
cancellation, and JSONL auditing. The allowlist, discovery, structured direct
execution, environment inheritance, status, cancellation, process-group
cleanup, and bounded persistent JSONL auditing are connected now. Dynamic
schema enumeration remains to be implemented. The canonical Skill and
agent-native discovery are connected now.
[`ADR 0002`](docs/adr/0002-host-capability-bridge.md)
now records the superseded arbitrary-shell bridge.

The guest proxy contract and its security consequences are documented in
[`ADR 0005`](docs/adr/0005-inherited-guest-proxy.md).

## Project layout

```text
src/
├── lib.rs                  Library entry point
├── main.rs                 Terminal application entry point
├── error.rs                Centralized error messages
├── agent/                  Agent-specific state and command adapters
├── cli/                    clap commands and natural agent entry points
├── host_bridge/            Authenticated host shell MCP bridge
├── preflight/              Host path resolution and checks
├── runtime/                Inspectable plans and container arguments
└── profile/
    ├── mod.rs              Profile module boundary
    ├── loader.rs           File reading, parsing, and validation pipeline
    ├── model.rs            Versioned profile data model
    ├── parser.rs           Side-effect-free parsing
    └── validation.rs       Fail-closed semantic validation
tests/           Cross-module, CLI, and canonical Skill contract tests
tests/fixtures/  Deterministic test inputs and expected outputs
skills/          Canonical agent Skill sources
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
