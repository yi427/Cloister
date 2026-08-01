# ADR 0002: Minimal host command bridge

- Status: Accepted for MVP, amended 2026-08-01
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
The bridge listens only on loopback. Manual `cloister host serve` usage keeps
the owner-only bearer-token file interface.

`cloister codex` enables the bridge by default as a core product capability.
For each invocation it:

1. generates a fresh bearer token in memory;
2. starts the bridge on `127.0.0.1:17834`;
3. asks Apple `container` to inherit the token by environment-variable name,
   without placing the value in command arguments;
4. injects a transient Streamable HTTP MCP configuration into Codex with
   `required = true`, an allowlist containing only `host.exec`, and per-call
   prompt approval; and
5. stops the bridge and discards the token when Codex exits.

The token is not written into persistent Codex configuration or mounted into
the guest as a file. `--no-host-bridge` disables the capability for one
invocation, and `--host-bridge-port` resolves a local port conflict.

The guest endpoint uses `host.container.internal`. Apple `container` must have
an explicit localhost DNS domain for that name. Cloister does not silently run
the privileged `container system dns create` operation.

There is no command allowlist, per-tool policy, confirmation flow, sandbox, or
path restriction in this version.

## Security boundary

`host.exec` is an explicit high-privilege escape hatch. An AI that possesses the
bridge token can run arbitrary commands and access anything available to the
macOS user running Cloister. The container does not protect the host from those
commands.

The bridge is enabled by default by product decision and must therefore be
described prominently and rendered in the inspectable runtime plan. Codex is
asked to prompt before each MCP call, but that prompt is not an isolation
boundary. Possession of the token is the actual bridge authorization boundary.
The token is available to the Codex process and can be inherited by guest
subprocesses; a process that obtains it can call the bridge without a Codex
approval prompt. Default bridge enablement therefore deliberately grants the
guest a path to the macOS user's authority.

Stronger command policy can be added only when real usage shows which
boundaries are needed.

## Deferred

Cloister does not yet create or repair the privileged Apple localhost DNS rule,
provide a command allowlist, or attach policy to individual host commands.
