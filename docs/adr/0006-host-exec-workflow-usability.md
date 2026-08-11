# ADR 0006: Host Exec workflow usability

- Status: Accepted, partially implemented
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

`host.list_commands` will add the canonical fixed Host working directory to its
read-only output. This makes relative Host command paths inspectable without
requiring a preliminary `pwd` command.

The field exposes metadata only. It does not add a request-provided `cwd`, allow
directory selection, or weaken the existing working-directory validation.

### Add bounded waiting to status reads

`host.exec_status` will accept an optional `wait_ms`. A positive value asks the
bridge to wait until new output is available after the supplied cursor, the
execution becomes terminal, or the requested interval expires.

The initial contract is:

- omission or `0` preserves the existing immediate status response;
- the server caps the effective wait at 30 seconds;
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

The workspace-routing portion is implemented and verified:

- the Skill frontmatter excludes shared workspace file operations;
- the Skill body describes the Guest/Host choice and rejects Base64 plus
  interpreter file transfer;
- a focused contract test preserves those instructions;
- the Skill validator and repository verification pass; and
- a real Claude session used Guest file tools successfully, with changes
  appearing immediately in the Host workspace.

Before this ADR is fully implemented:

- test that a real macOS-native command still selects Host Exec after the Skill
  change;
- add focused discovery tests for the canonical Host working directory;
- add status-wait tests for new output, terminal completion, expiration, the
  service-side cap, unknown execution IDs, and cursor behavior;
- update the canonical Skill, README, and ADR 0004 references for the connected
  tool schema; and
- run `make verify` plus real Codex and Claude Host Exec acceptance through
  Apple `container`.
