# ADR 0002: Minimal host command bridge

- Status: Accepted for prototype
- Date: 2026-07-29

## Context

AI agents run inside a Linux environment, while some development tasks require
native macOS tools. Cloister needs a small, natural escape hatch without
designing a full capability and policy system before the basic AI workflow
exists.

## Decision

Cloister provides one authenticated MCP tool:

```text
host.exec(command)
```

The tool runs `command` through `/bin/zsh -lc` as the macOS user running the
bridge and returns stdout, stderr, the exit status, and execution duration.
The prototype listens only on loopback and uses an owner-only bearer-token file.

There is no command allowlist, per-tool policy, confirmation flow, sandbox, or
path restriction in this version.

## Security boundary

`host.exec` is an explicit high-privilege escape hatch. An AI that possesses the
bridge token can run arbitrary commands and access anything available to the
macOS user running Cloister. The container does not protect the host from those
commands.

The bridge must therefore be opt-in and described honestly. Stronger controls
can be added only when real usage shows which boundaries are needed.

## Deferred

The prototype does not yet configure the Apple container guest-to-host network
path or inject the MCP endpoint and token into an AI agent.
