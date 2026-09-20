# ADR 0008: Independent Agent software and concurrent sessions

- Status: Accepted, implemented; runtime smoke checks passed
- Updates: ADR 0001, ADR 0003, ADR 0006, ADR 0007

## Decision

Profile V7 makes Agent program selection explicit: `agent.codex`, `agent.claude`
accept `latest` or an exact stable version, and `agent.update_interval_hours`
sets the automatic check interval. `0` checks every launch. Older schemas are
rejected without migration. Node/Rust and Cloister integration remain in the
versioned base image; Agent packages are downloaded only when needed.

Cloister owns updates rather than allowing running Agents to rewrite their
shared program files. The official npm registry distributes both Linux ARM64
packages. A dedicated installer container has a read-only root, ephemeral home
and npm cache, and one writable staging mount. It has no project, Agent state,
or bridge token. Explicit proxy inheritance applies to preparation too.

The host CLI serializes preparation with a kernel file lock. Completed versions
are separate directories; inventory metadata is atomically replaced only after
installation and an executable smoke check. Only the selected directory is
mounted read-only in the session. Shared Agent credentials are a separate policy:
isolated credentials do not imply repeated program downloads.
Each image reference retains its own latest selection and update-check timestamp,
so switching images preserves offline launch and independent check intervals.

An automatic update check failure may reuse a previously installed version, with
a warning. No cached version, an explicit update failure, or corrupt metadata
fails launch. Exact-version selection supports rollback without changing the
default latest selection. Offline means no software checks/downloads, not blocked
internet. Dry-run and check remain read-only. Old installations are retained to
avoid removing files used by active sessions. Garbage collection is deferred.

Every session binds loopback port 0 by default, then injects the actual bound
port. The same listener is retained, avoiding a probe-then-bind race. Tokens and
execution records remain session-local. An explicitly occupied port fails.
SIGINT/SIGTERM/SIGHUP request cleanup of the specific named container, followed by
bridge shutdown. SIGKILL and host/runtime failures need manual recovery.

## Compatibility and trust

CLI/base-image release pairing remains exact, but no longer implies an Agent
version pairing. Latest packages may change MCP flags or skill discovery. Release
acceptance records exact Agent versions; fixed selectors provide recovery. An
unrecognized future release must not be described as certified compatible.

The cache is protected from normal guest-session writes, not from an allowed
Host Exec command with sufficient macOS permissions. Official package code and
install scripts are trusted inside the preparation container. npm integrity
checks do not establish that publisher code is harmless. Metadata rejects unsafe
paths, symlinks, invalid selectors, and unsupported inventory schemas.

Installation output is suppressed because package-manager failures may expose
proxy values. Cloister reports installation phase, selected version, cache path
in plans, and bounded non-secret failure messages. Existing host credentials,
Agent directories, npm configuration, and privileged sockets are never mounted.

## Validation

Deterministic tests cover first use, cache reuse with isolated state, concurrent
first launches, different bridge endpoints/tokens, read-only software mounts,
installer mount scope, atomic upgrade and rollback, automatic fallback, explicit
update failure, interruption, locks, unsafe paths, and Profile parsing. Real
Apple-container installation, cache reuse, read-only enforcement and concurrent
CLI smoke checks passed; see [acceptance record](../testing/agent-software-v7.md).
Authenticated coding and live Agent-to-MCP acceptance remain release requirements.
