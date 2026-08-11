# ADR 0006: Host Exec workflow usability

- Status: Accepted, implemented
- Date: 2026-08-11
- Updates: ADR 0004

## Context

ADR 0004 defines Host Exec as an explicit bridge from the Linux Guest to
Profile-allowed macOS executables. The selected Host workspace is independently
mounted read-write at Guest `/workspace`, so normal project file operations do
not need that bridge.

Real Claude usage exposed an instruction gap. When asked to create or modify a
workspace file, Claude encoded the complete content as Base64 and called an
allowed Host Python through `host.exec` with `python -c`. It did this to avoid a
suspected Guest-to-Host synchronization failure, even though Guest file tools
and the live workspace mount were working correctly.

That workaround has undesirable properties:

- ordinary file changes trigger Host execution approval;
- Base64 adds about 33% payload overhead and makes calls difficult to inspect;
- a small edit becomes a whole-file rewrite;
- an interpreter receives much broader Host authority than file editing needs;
  and
- the behavior obscures the distinction between Guest workspace access and
  Host-native execution.

The same testing also showed two smaller usability gaps in the asynchronous
Host Exec contract. The fixed Host working directory is not discoverable, and
long-running commands require repeated short `host.exec_status` polling even
when the caller only needs to wait briefly for more output or completion.

## Decision

### Keep shared workspace file operations in the Guest

The canonical `host-exec` Skill must make the capability choice explicit:

- read, write, list, and patch `/workspace` with Guest file tools;
- treat Guest `/workspace` as the live read-write view of the selected Host
  workspace;
- do not use `host.exec`, Base64, Python, or another allowed interpreter as a
  workspace file-transfer mechanism;
- use Host Exec only when a task requires a Profile-allowed macOS executable;
  and
- when a Host executable consumes workspace files, prefer paths relative to
  its fixed Host working directory.

A file outside `/workspace` is outside the shared workspace. The Skill must
report that boundary instead of turning an allowed interpreter into an implicit
Host file API.

This is model guidance, not a server-enforced sandbox. If a Profile grants an
interpreter `arguments = "any"`, the bridge cannot determine whether that
process later writes a file. Hard parameter restrictions require a separate
Profile and authorization design.

### Expose the fixed Host working directory through discovery

`host.list_commands` adds the canonical fixed Host working directory to its
read-only output. This makes relative Host command paths inspectable without
requiring a preliminary `pwd` command.

The field exposes metadata only. It does not add a request-provided `cwd`, allow
directory selection, or weaken the existing working-directory validation.

### Add bounded waiting to status reads

`host.exec_status` accepts an optional `wait_ms`. A positive value asks the
bridge to wait until new output is available after the supplied cursor, the
execution becomes terminal, or the requested interval expires.

The initial contract is:

- omission or `0` preserves the existing immediate status response;
- the server caps the effective wait at 30 seconds;
- at most 32 blocking status waits may be active, and excess waits fail with a
  typed capacity error without blocking cancellation;
- expiration returns the current state and cursor without cancelling the
  process;
- client disconnect does not cancel the process; and
- status waiting remains read-only and does not require another execution
  approval.

The fixed 100 ms inline window in `host.exec` remains unchanged. Waiting belongs
to `host.exec_status` so an already-approved execution is not coupled to a long
initial tool call.

## Compatibility and versioning

These changes do not alter Profile V6. The Host working directory is an
additive discovery response field, and `wait_ms` is an optional status request
field. Existing clients that omit it retain the current behavior.

The versioned `host.exec` request remains DSL version 1 because its command and
argument authorization contract does not change. A released CLI and Guest
image remain a tested pair; the updated canonical Skill ships in the image.

## Security consequences

- Workspace writes continue to have the full consequences of a live read-write
  bind mount. Guest changes are not protected from modification or deletion.
- The Skill reduces unnecessary Host authority use but cannot override an
  explicitly broad Profile allow entry.
- Discovery will reveal the canonical selected Host workspace path to the
  connected Guest. It must not reveal environment values or unrelated Host
  paths.
- Bounded status waiting consumes one read-only request while waiting but does
  not create another execution or extend execution authority.
- A bridge-wide limit of 32 blocking status waits bounds the new long-lived
  request resource without limiting immediate reads or `host.exec_cancel`.
- Cloister still does not mount Host credential directories, home directories,
  or privileged sockets implicitly.

## Deferred work

This decision does not add:

- `host.read_file`, `host.write_file`, `host.list_dir`, patch, or batch file
  tools for paths outside the shared workspace;
- subcommand or argument-pattern policy beyond `arguments = "any"`;
- a model-selected Host working directory;
- stdin or PTY support; or
- automatic Codex, Claude, CLI, or Guest image updates.

Dedicated Host file tools require an explicit root policy, canonical path and
symbolic-link escape handling, size limits, atomic-write semantics, approval,
and content-free auditing. Fine-grained argument policy requires a later
versioned Profile contract. Agent updates require a separate compatibility and
rollback decision.

## Validation

The workspace-routing, working-directory discovery, and bounded status-wait
portions are implemented and locally verified:

- the Skill frontmatter excludes shared workspace file operations;
- the Skill body describes the Guest/Host choice and rejects Base64 plus
  interpreter file transfer;
- a focused contract test preserves those instructions;
- discovery returns the same canonical Host working directory used by
  `host.exec`;
- omitted or zero waits return immediately, positive waits return for retained
  output, terminal completion, or expiration, and oversized waits are capped;
- unknown execution IDs fail before waiting, a dropped status client does not
  cancel the process, and cursor filtering remains incremental;
- 32 blocking status waits exhaust the dedicated capacity while immediate
  reads and cancellation remain available;
- the Skill validator and repository verification pass;
- a real Claude session discovered `/Volumes/Home/project/Cloister` and a
  Profile-allowed Host Python reported the same value from `os.getcwd()`,
  without an extra `pwd`, a custom working directory, or file changes;
- a real Claude session used Guest file tools successfully, with changes
  appearing immediately in the Host workspace; and
- the same session discovered the Profile-allowed Host Python and invoked it
  through `host.exec`, reporting `Darwin`, `arm64`, and Python 3.13.15 without
  confusing it with a Guest file operation; and
- real Claude acceptance through Apple `container` observed two output-driven
  wakes while the execution remained running, a terminal wake with no repeated
  chunks, an immediate zero-wait snapshot, and a 300 ms expiration followed by
  a terminal wake. Every call advanced from the latest cursor, only one status
  wait was active at a time, and status reads required no additional approval.
