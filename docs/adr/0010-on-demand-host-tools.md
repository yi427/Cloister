# ADR 0010: Explicit on-demand Host tools

- Status: Accepted, implemented
- Date: 2026-10-10
- Updates: ADR 0004 and ADR 0006

## Context

An immutable executable allowlist requires a user to edit the Profile and
restart a session whenever an Agent needs another installed macOS tool. The
requested workflow grants broad host tool access once, then relies on clear
Guest-first instructions to reduce unnecessary Host operations.

## Decision

Profile V8 requires `host.exec.mode`, either `allowlist` or `on-demand`. V7
continues to load with allowlist behavior; it cannot enable on-demand. New
Profiles default to explicit allowlist mode. `init --host-exec-mode on-demand`
opts into broad access and displays that authority in the creation summary.
Existing Profiles and running sessions are not modified.

The bridge retains its authenticated fixed-workspace execution protocol and
adds read-only `host.find_command`. `host.list_commands` reports the mode and
pinned commands. Allowlist discovery exposes only allowed names. On-demand
discovery can also resolve bare executable names from the immutable Host
startup environment's PATH, ignoring empty and relative directories. It never
runs the executable, a shell, `which`, or a version probe. Pinned entries take
precedence. Arbitrary model-supplied executable paths and environment overrides
remain rejected.

Successful PATH discovery is cached per bridge. Reuse checks that the path
remains executable and resolves to the same canonical target. A changed target
requires restarting the bridge. This is not tamper-resistant program identity;
same-path file replacement, subsequent filesystem races, and code loaded by a
permitted process are not contained.

`host.exec` adds an optional `reason` under DSL version 1. In on-demand mode it
must be nonblank, at most 240 characters, and free of control characters,
including for pinned commands. This keeps the purpose in the MCP call without
persisting it in audit logs or Debug output. It checks shape, not truth.

The MCP methods remain fixed. Program discovery does not register a new MCP
method, rewrite a Profile, or require tool-list refresh notifications.

On-demand Codex launches select automatic approval for Cloister Host tools.
Claude launches add exact permission rules for those five tools, and the
bridge omits its execution-interaction metadata. No global permission bypass
is applied. Allowlist launches retain the execution prompt. Client deny/ask
policies and user-provided overrides remain outside Cloister's guarantee.
Claude's permission syntax follows its [official CLI reference](https://code.claude.com/docs/en/cli-reference)
and [MCP permission rules](https://code.claude.com/docs/en/permissions).

## Boundaries and behavior

On-demand grants broad macOS-user execution authority, including interpreters,
shells, and build tools. The Guest VM, fixed working directory, purpose text,
and Agent approval policies do not contain Host process effects. The selected
workspace is still a live read-write mount, and internet access is unchanged.

The canonical Skill and server/tool instructions require Guest tools for
ordinary workspace work, Host tools only for macOS capabilities or missing
Guest executables, an explanation before each Host operation, reuse of
discovery, bounded status waits, and cancellation of unneeded jobs. These are
behavioral instructions, not enforcement of least privilege. Existing
execution/output/concurrency limits and content-free auditing continue to apply.

## Validation

Deterministic tests cover versioned parsing, legacy allowlist behavior,
mode-specific discovery, no discovery execution, PATH ordering, pinned
overrides, purpose validation, changed symlink targets, literal arguments,
authenticated MCP execution, audit secrecy, both Agent launch policies,
disabled bridges, and visible runtime plans. Authenticated Agent behavior must
be checked separately using the [acceptance procedure](../testing/on-demand-host-tools.md).
