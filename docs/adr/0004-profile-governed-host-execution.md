# ADR 0004: Profile-governed asynchronous host execution

- Status: Accepted, partially implemented
- Date: 2026-08-05
- Supersedes: ADR 0002

Profile V6 later adds the independent guest proxy policy defined in ADR 0005
while retaining this Profile V5 Host Exec contract unchanged.

## Context

ADR 0002 deliberately shipped the smallest useful bridge: an authenticated
`host.exec(command)` tool that passes an arbitrary string to `/bin/zsh -lc`.
That established the end-to-end Apple `container`, MCP, authentication, and
agent-adapter path, but possession of the bridge token granted the macOS user's
full shell authority.

Cloister now has enough real workflow to define a narrower and more inspectable
contract. The bridge needs to tell an agent which host commands are available,
enforce the same policy independently of model behavior, support long-running
builds without an arbitrary global timeout, and retain an audit trail without
persisting command output or secrets.

The first connected slice now enforces the Profile allowlist, exposes
`host.list_commands`, accepts structured `version + command + args` requests,
inherits the complete trusted host environment, and starts the selected
absolute executable directly without `/bin/zsh -lc`. The asynchronous execution
manager, incremental status, cancellation, and process-group cleanup are now
connected. Bounded persistent JSONL auditing is also connected. Dynamic command
enumeration in JSON Schema remains a planned portion of this accepted design.
The canonical Skill and its Codex and Claude discovery paths are connected.

## Decision summary

Cloister keeps one authenticated MCP server, `cloister_host`. It exposes:

- `host.list_commands` discovers the commands allowed by the loaded Profile;
- `host.exec` validates and starts one structured command invocation;
- `host.exec_status` reads the state and incremental output of an invocation;
- `host.exec_cancel` terminates an invocation and its descendants.

Cloister will also provide one canonical `host-exec` Skill. Agent adapters may
expose that Skill through their native discovery mechanism, but there must be
one maintained source of instructions rather than separate Codex and Claude
policy documents.

The Profile is the policy authority. The Skill and the MCP tool schema help the
model use the capability correctly, but neither is an enforcement boundary.
Every `host.exec` request is validated by the host bridge against an immutable
Profile snapshot loaded when the bridge starts.

The first restricted version allows arbitrary argument vectors for an allowed
executable. This intentionally starts with a broad, explicit permission and
leaves argument-specific rules for a later schema.

## Security claim

This design changes the bridge from arbitrary shell access to an explicit,
discoverable executable allowlist. It improves least privilege, reviewability,
and auditability. It is not an operating-system sandbox.

An allowed program still runs as the macOS user that started Cloister. It may
read or modify any data available to that user, open network connections, and
start other programs. Restricting the initial working directory does not
restrict the files that the program can subsequently access.

Some allowlist entries are effectively general host-code-execution grants when
arbitrary arguments are accepted. Examples include:

- `python3`, because `python3 -c` can execute arbitrary host code;
- `cargo`, because `cargo run`, build scripts, and procedural macros execute
  host code;
- `rustc`, because it can create a new executable that another allowed command
  may run;
- `xcodebuild`, because an Xcode project may contain build-phase scripts; and
- wrapper tools such as `xcrun`, because they can dispatch to many other tools.

Cloister must describe these permissions honestly. It must not claim that an
allowlisted interpreter or build system is contained merely because the
initial executable was named in the Profile.

The existing bearer-token boundary remains. Agent approval prompts are an
interaction policy, not an isolation boundary. A guest process that obtains the
token can call any enabled bridge tool directly. Server-side policy validation
therefore remains mandatory for every request.

## Proposed Profile V5 contract

Host execution policy belongs to the environment Profile, not to an individual
project. Profile V5 adds an explicit `[host.exec]` section:

```toml
schema_version = 5
name = "cloister"

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

[agent]
state = "shared"

[host.exec]
enabled = true

[host.exec.environment]
mode = "inherit-all"

[[host.exec.allow]]
name = "xcodebuild"
executable = "/usr/bin/xcodebuild"
description = "Build and test Xcode projects"
arguments = "any"

[[host.exec.allow]]
name = "git"
executable = "/usr/bin/git"
description = "Inspect and modify Git repositories"
arguments = "any"
```

The example command entries illustrate the schema; they are not a decision to
install those entries in every generated Profile.

`enabled` is required. When it is false, natural agent entry points do not
start or inject the Host Bridge unless a future explicit override is designed.
`--no-host-bridge` remains a one-invocation reduction of authority and cannot
enable a Profile-disabled bridge.

An enabled policy may contain an empty allowlist. This explicitly authorizes no
host commands and is the safe default written when the user selects none in
`init`. `init` must not infer executable permissions from `PATH` without an
explicit command name entered by the user.

The initial `arguments` policy accepts only `"any"`. Keeping the field explicit
makes the broad permission visible and reserves a versioned location for later
argument rules. Concurrency, output, and per-command runtime limits are not
part of Profile V5; adding them requires a later schema version.

### Profile validation

Profile parsing continues to reject unknown fields and unsupported schema
versions. Profile V5 validation must additionally require that:

- command names are non-empty, unique stable identifiers;
- executable paths are absolute;
- descriptions are non-empty;
- `arguments` is exactly `"any"` in Profile V5;
- the environment mode is exactly `"inherit-all"` in Profile V5.

Static parsing cannot prove that a host executable exists. `cloister check`
and bridge startup must resolve every configured path, permit an explicitly
configured symbolic-link or toolchain shim, and require its resolved target to
be a regular executable file. The runtime plan and audit event should preserve
both declared and resolved paths when they differ.

Profile V4 was rejected when V5 was introduced. Profile V6 now retains this
Host Exec contract and rejects V5 because it adds the required guest proxy
policy from ADR 0005. Cloister is still in development and will not add a
compatibility alias or silent migration layer. `init` writes the current
contract and continues to refuse overwriting an existing Profile.

## Environment construction

The request DSL cannot provide environment variables. The bridge constructs the
environment from the Profile before starting the process:

```text
Command::new(executable)
    .env_clear()
    .envs(selected_profile_environment)
    .args(request.args)
```

Profile V5 supports only `inherit-all`. Host execution already grants the
selected executable the macOS user's authority, so full environment inheritance
does not produce a separate warning. `init`, `check`, and the runtime plan still
report the selected mode explicitly.

Full inheritance includes variables such as `SSH_AUTH_SOCK`, credential tokens,
`DYLD_*`, `LD_*`, `PYTHONPATH`, `NODE_OPTIONS`, `BASH_ENV`, and `ENV` when they
exist in Cloister's environment. This is deliberate behavior, not filtering or
containment. Environment values must never be returned by
`host.list_commands`, printed in a runtime plan, persisted in an audit log, or
included in debug output.

An absolute executable path prevents the bridge itself from resolving the
initial command through `PATH`. If `PATH` is inherited, however, the allowed
program and its build scripts may use it to find child programs. This is part
of the permission granted to that executable, not a containment guarantee.

## Request DSL

The connected structured request format is a JSON object:

```json
{
  "version": 1,
  "command": "xcodebuild",
  "args": ["-project", "Example.xcodeproj", "build"]
}
```

`command` selects an allow entry by its stable Profile name. It is not a path
and is never interpreted by a shell. `args` is passed directly as the child
argument vector, so shell operators, substitutions, redirections, and quoting
have no special meaning.

The model cannot select `cwd` in version 1. Natural agent launches use the
canonical selected workspace root, including an explicit `--workspace`; manual
`cloister host serve` uses its canonical launch directory. A later request
version may add a workspace-relative directory after escape and symlink rules
are implemented. This working-directory restriction would not prevent an
allowed program from opening an absolute host path itself.

The request contains no executable path, environment object, shell string, or
required timeout. A future DSL change requires a new `version` value and must
remain fail-closed.

## MCP tools

### `host.list_commands`

This read-only tool accepts no policy input. It returns:

- DSL version;
- the canonical fixed Host working directory selected for this bridge;
- allowed command names and descriptions;
- the argument policy for each command;
- environment mode and variable names, never values;
- whether audit logging is active.

The generated JSON Schema for `host.exec.command` should also use an `enum` of
the loaded command names. This improves tool selection but is not an
authorization check. `host.exec` must independently look up and validate the
command on every call.

### `host.exec`

`host.exec` validates the request, starts the direct child process in a new
process group, registers it with the execution manager, and returns an
`execution_id`.

The server may wait for a small fixed inline response window. If the process
finishes during that window, `host.exec` returns its final state and available
output. Otherwise it returns `running`. The inline response window is not an
execution timeout and never terminates a process.

Callers must accept either state:

```json
{
  "execution_id": "exec_7k2...",
  "state": "running",
  "next_cursor": 0
}
```

Starting a host process is the authority-increasing operation. Agent adapters
should retain the existing per-call user-interaction behavior for `host.exec`.

The connected implementation uses a 100 ms inline response window. It has no
execution timeout.

### `host.exec_status`

This read-only tool accepts an `execution_id`, an optional output cursor, and an
optional bounded wait:

```json
{
  "execution_id": "exec_7k2...",
  "cursor": 0,
  "wait_ms": 10000
}
```

A positive `wait_ms` waits until retained output is available after the cursor,
the execution becomes terminal, or the interval expires. Omission or `0`
returns an immediate snapshot. The bridge caps the effective wait at 30 seconds
and permits at most 32 concurrent blocking status waits. A request above that
capacity returns a typed error; immediate status reads and `host.exec_cancel`
do not consume this capacity.

It returns the current state and only output chunks after that cursor:

```json
{
  "execution_id": "exec_7k2...",
  "state": "running",
  "duration_ms": 38642,
  "exit_code": null,
  "chunks": [
    {
      "cursor": 1,
      "stream": "stdout",
      "text": "Compiling cloister v0.1.0\n"
    }
  ],
  "next_cursor": 1,
  "stdout_bytes": 26,
  "stderr_bytes": 0,
  "output_truncated": false
}
```

Output is text for agent usability; invalid UTF-8 is replaced explicitly. Byte
counts refer to raw process output. Chunk order represents bridge read order and
must not be described as an exact ordering guarantee between independent
stdout and stderr pipes.

Terminal states are `completed`, `failed`, and `cancelled`. A completed process
reports its exit code when the platform provides one. Evicted or unknown
execution IDs return a typed error rather than being treated as still running.

Status reads should not require another execution approval. Expiration returns
the current snapshot without cancelling the process, and disconnecting a
status client does not cancel it. The Skill should keep one status wait in
flight with a recommended 10-second wait, then repeat only while the execution
remains `running`.

### `host.exec_cancel`

This tool accepts an execution ID owned by the current bridge instance. It
requests cancellation and returns the resulting or transitional state.

The execution supervisor sends a graceful termination signal to the complete
process group, waits a short bounded grace period, and then forcefully kills
remaining members. Killing only the direct child is insufficient for build
systems that start compilers, scripts, or test processes.

Cancellation reduces existing authority and should not require a second user
approval. It cannot address a process that belongs to another bridge instance.

## Execution manager

The bridge manages executions in memory; it does not use shell `&`, the shell
`bg` builtin, `nohup`, or a detached host daemon.

One `ExecutionManager` is created in `serve()` and shared by every RMCP service
instance:

```text
Arc<ExecutionManager>
    executions: HashMap<ExecutionId, ExecutionRecord>

ExecutionRecord
    immutable request and policy metadata
    start time
    state
    bounded output chunks
    cancellation token
```

`host.exec` launches the process with `spawn()`. Tokio tasks supervise the
child, drain stdout and stderr concurrently, update the record, and notify the
inline waiter or later status calls. Registry locks must not be held across
process or I/O awaits.

Executions are session-scoped:

- a bridge restart does not restore jobs;
- stopping Cloister cancels and reaps all running process groups;
- an MCP request ending does not cancel a successfully registered execution;
- completed records remain available only within bounded in-memory retention;
  and
- persistent JSONL audit events survive, but captured process output does not.

Bridge shutdown must wait for execution cleanup before reporting success. This
extends the existing server lifecycle rule to the host processes created by the
server.

## Duration and resource limits

Elapsed time alone is not treated as suspicious. Builds and test suites may run
for hours. Therefore:

- there is no global default execution timeout;
- the MCP request lifetime is kept short by returning an execution ID.

The execution manager permits at most eight concurrent processes and 32
concurrent blocking status waits, retains at most 1 MiB of output per execution,
and retains at most 128 execution records. It evicts the oldest terminal records
as needed. Reaching the output limit marks the status as truncated but does not
stop the process. The cancellation grace period is two seconds, followed by a
forceful process-group kill and a bounded one-second cleanup wait. Bridge
shutdown allows four seconds total for all registered executions to reach a
terminal state. Profile-governed concurrency, output, and maximum runtime
limits are deferred to a later schema.

These controls prevent accidental bridge resource exhaustion. They do not
limit CPU, memory, filesystem, or network activity performed by an allowed host
program. Such host-level containment is deferred.

## Audit log

The bridge writes structured JSON Lines events on the host for authorized,
policy-denied, process-start-failed, cancelled, failed, and completed execution
attempts. Each event has an audit schema version and enough stable metadata to
correlate the lifecycle:

- timestamp, event kind, request ID, and execution ID when assigned;
- agent, Profile name, and the canonical workspace used as the initial working
  directory;
- allowlist command name and declared/resolved executable path;
- argument count and whether argument values were redacted;
- environment mode and variable names, never values;
- outcome, exit code when available, and duration;
- stdout and stderr byte counts; and
- output truncation and configured-limit information.

The audit schema is independently versioned at V1. The first version does not
persist raw argument values, stdout, stderr, environment values, bearer tokens,
or agent credentials. Arbitrary arguments may contain secrets, and the bridge
cannot reliably infer which positions are sensitive. A later Profile version
may add an explicit per-command argument redaction contract.

The `execution_started` event is committed before spawning the host process so
that audit failure prevents host side effects. A spawn failure therefore has a
correlated `execution_started` event followed by `execution_failed`.

`host.exec_status` output remains available only in the bounded in-memory job
record. The audit destination is
`${XDG_STATE_HOME:-~/.local/state}/cloister/audit/host-exec.jsonl`. The
`cloister` and `audit` directories must be owner-only directories with mode
`0700`; the active log, rotated log, and lock file must be owner-only regular
files with mode `0600` and exactly one hard link. Cloister rejects symbolic
links, wrong ownership, and broader permissions rather than silently repairing
existing paths. Existing segments larger than the per-file limit are also
rejected rather than silently truncated.

The active file and `host-exec.jsonl.1` are each limited to 10 MiB, for a total
JSONL bound of 20 MiB. Rotation happens before an append would exceed the
per-file limit. Multiple Bridge processes coordinate every size check, rotation,
single-line append, and data flush with an inter-process lock. Bridge shutdown
cleans up executions before draining and joining the audit writer. Dry runs and
`check` resolve and display the path without creating it.

These logs survive ordinary Bridge and machine restarts, but they are
observational rather than tamper-proof. An allowed host command runs as the same
macOS user and can modify or delete them.

`host.list_commands` and status polling are not execution-attempt audit events.
Requests rejected by the HTTP Bearer-authentication middleware are not written
to the V1 audit log, and bearer values are never logged.

## Canonical Skill behavior

The `host-exec` Skill is guidance for both Codex and Claude. It should remain
short and procedural:

1. call `host.list_commands` before the first host operation in a session;
2. select only a command returned by that tool;
3. submit a structured request without shell syntax or environment injection;
4. accept either an inline final result or a running execution ID;
5. wait on `host.exec_status` with the latest cursor for long operations;
6. call `host.exec_cancel` when the operation is no longer wanted; and
7. report denied policy rather than trying to bypass it with another command.

The Profile is immutable for one bridge lifecycle, so one successful discovery
call is sufficient unless the agent reconnects to a new bridge. The Skill must
state that command availability is an explicit host permission and that an
allowed interpreter or build tool may still be high privilege.

The semantics for the connected four-tool surface are maintained in
[`skills/host-exec/SKILL.md`](../../skills/host-exec/SKILL.md). The image owns
one read-only canonical source. With the Host Bridge
enabled, Codex receives a temporary `$HOME/.agents/skills` symlink and Claude
receives an image-owned `--add-dir` containing a `.claude/skills` symlink.
Neither adapter writes Skill files into persistent agent state, and disabling
the bridge also disables these discovery paths. Because Claude gives its
persistent Skill directory precedence over additional project directories,
Cloister rejects an existing shared-state `skills/host-exec` entry before
starting the bridge. It does not overwrite the user entry or silently run with
an ambiguous instruction source; the check is not applied when the bridge is
disabled.

## CLI behavior

`init` creates an enabled current-Profile policy with `inherit-all` environment mode
and continues to refuse overwriting an existing target. Its allowlist remains
empty when the user accepts the default. The user may explicitly enter a
comma-separated list of bare command names; only those names are resolved from
absolute directories in the current `PATH`. Lookup never invokes a shell or
`which`, invalid or duplicate entries cause the whole list to be requested
again, and the declared path plus canonical target are shown before the final
Profile confirmation. The selected `PATH` entry is stored so symlink-based
dispatch such as tool shims remains intact, while `arguments = "any"` and a
description are generated explicitly for every selected entry.

`check` remains read-only and adds checks for:

- current Profile Host Exec policy validity;
- declared executable existence, canonical symlink resolution, regular-file
  type, and execute permission bits;
- the audit directory and files' safe type, ownership, permissions, link count,
  and per-segment size when they exist; and
- explicit `inherit-all` environment mode.

For a stale declared path, `check` may search the current `PATH` for the
declared filename and print an explicit replacement suggestion. Lookup is
internal, never invokes a shell or `which`, and ignores empty or relative
`PATH` entries. The check never rewrites the Profile.

The runtime plan must show, without secrets:

- whether the bridge is enabled;
- allowed command names and declared/resolved paths;
- `arguments = "any"`;
- environment mode without environment values;
- asynchronous DSL version and tool names; and
- audit status and destination.

The runtime plan must continue to say that the bridge runs programs with the
macOS user's permissions. It must not imply host containment.

## Acceptance criteria

Implementation is not complete until focused tests cover:

- current Profile parsing, validation, unknown fields, and prior-version rejection;
- duplicate command names and relative executable paths;
- allowed and denied command lookup;
- literal argv handling proving that shell syntax is not evaluated;
- workspace-relative cwd resolution and symbolic-link escape rejection;
- complete inherited environment injection without request-provided overrides;
- fast inline completion and long-running status polling;
- incremental mixed stdout/stderr output and truncation without pipe blockage;
- concurrency rejection;
- cancellation and complete descendant process-group cleanup;
- bridge shutdown cleanup and completed-record eviction;
- JSONL audit events with secret values and process output absent;
- dynamic discovery and `host.exec.command` schema enumeration;
- no approval requirement for list/status/cancel where each agent supports it;
  and
- real Codex and Claude end-to-end use through Apple `container`.

`make verify` and the staged-snapshot pre-commit hook remain the required
repository checks.

## Deferred and open decisions

The current Profile does not attempt argument-pattern rules, configurable environment
filtering, per-project policy, host-level CPU or memory containment, network
restrictions, persistent jobs, interactive stdin, PTY programs, or
Profile-governed execution limits.

The remaining decisions are dynamic command schema enumeration and real Codex
and Claude verification of the four-tool approval behavior.
