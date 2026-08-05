# ADR 0002: Minimal host command bridge

- Status: Superseded by ADR 0004 on 2026-08-05
- Date: 2026-07-29

## Context

AI agents run inside a Linux environment, while some development tasks require
native macOS tools. Cloister needs a small, natural escape hatch without
designing a full capability and policy system before the basic AI workflow
exists.

## Historical decision

Cloister provides one authenticated MCP tool:

```text
host.exec(command)
```

The tool runs `command` through `/bin/zsh -lc` as the macOS user running the
bridge and returns stdout, stderr, the exit status, and execution duration.
The bridge listens only on loopback. Manual `cloister host serve` usage keeps
the owner-only bearer-token file interface.

`cloister codex` and `cloister claude` enable the bridge by default as a core
product capability. For each invocation Cloister:

1. generates a fresh bearer token in memory;
2. starts the bridge on `127.0.0.1:17834`;
3. asks Apple `container` to inherit the token by environment-variable name,
   without placing the value in command arguments;
4. injects a transient Streamable HTTP MCP configuration: Codex receives
   `required = true`, an allowlist containing only `host.exec`, and per-call
   prompt approval; Claude receives an inline `--mcp-config` with
   `alwaysLoad = true` and an environment-backed authorization header;
5. advertises `host.exec` with Claude's
   `anthropic/requiresUserInteraction = true` metadata; and
6. stops the bridge and discards the token when the agent exits.

The token is not written into persistent agent configuration or mounted into
the guest as a file. `--no-host-bridge` disables the capability for one
invocation, and `--host-bridge-port` resolves a local port conflict.

The guest endpoint uses `host.container.internal`. Apple `container` must have
an explicit localhost DNS domain for that name. Cloister does not silently run
the privileged `container system dns create` operation.

There was no enforced command allowlist, per-tool policy, confirmation flow,
sandbox, or path restriction in this version.

## Security boundary

`host.exec` is an explicit high-privilege escape hatch. An AI that possesses the
bridge token can run arbitrary commands and access anything available to the
macOS user running Cloister. The container does not protect the host from those
commands.

The bridge is enabled by default by product decision and must therefore be
described prominently and rendered in the inspectable runtime plan. Codex is
configured to prompt before each MCP call. Claude's pinned version recognizes
the tool metadata and requires a human prompt on every call. Neither prompt is
an isolation boundary. Possession of the token is the actual bridge
authorization boundary. The token is available to the agent process and can be
inherited by guest subprocesses; a process that obtains it can call the bridge
without an agent approval prompt. Default bridge enablement therefore
deliberately grants the guest a path to the macOS user's authority.

Stronger command policy can be added only when real usage shows which
boundaries are needed.

## Successor

[`ADR 0004`](0004-profile-governed-host-execution.md) replaces this runtime with
a Profile V5 executable allowlist, structured argument vectors, environment
policy, and command discovery. Those enforcement pieces are connected. Its
asynchronous execution status, cancellation, Skill, dynamic schema enumeration,
and JSONL auditing remain incomplete.

## Deferred

Cloister does not yet create or repair the privileged Apple localhost DNS rule.
The successor does not yet implement asynchronous job management or persistent
audit storage.
