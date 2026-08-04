# ADR 0003: Natural agent entry point and shared state

- Status: Accepted for MVP, amended 2026-08-05
- Date: 2026-07-29

## Context

Requiring a project-specific Profile path and a complete container command for
every session makes the normal AI workflow feel like a container-runtime
wrapper. The common action should instead be running an agent in the current
project.

Agent login, configuration, sessions, and skills also need to survive an
ephemeral container. Mounting the host's existing `~/.codex` or `~/.claude`
would expose state that Cloister did not create or select.

## Decision

The high-level supported-agent workflows are:

```text
cd <project>
cloister codex
cloister claude
```

Cloister does not expose a general-purpose `run` command. Generic Apple
container planning and execution remain internal implementation details used by
agent-specific commands.

It uses the current directory as `/workspace` unless `--workspace` selects
another directory, reuses the configured OCI image, removes the container after
exit, and persists shared state in a separate Cloister-managed directory for
each agent:

```text
~/.local/share/cloister/agents/codex
~/.local/share/cloister/agents/claude
```

The Codex directory is mounted at `/cloister/agents/codex` and exposed through
`CODEX_HOME`. The Claude directory is mounted at `/cloister/agents/claude` and
exposed through `CLAUDE_CONFIG_DIR`. Each selected directory is restricted to
owner-only host permissions, and Cloister rejects a symbolic link at the
state-directory boundary.

Profile selection uses this order:

1. an explicit `--profile`;
2. `~/.config/cloister/profile.toml`.

There is no embedded fallback Profile. A missing default file is an explicit
configuration error.

`XDG_CONFIG_HOME` and `XDG_DATA_HOME` replace their respective home-relative
base directories when set. Profile V4 controls image, resources, guest
settings, explicit default networking, and a generic `[agent]` state policy.
That policy is either `shared` or `isolated` and applies to every supported
agent, while each agent receives a separate Cloister-managed state directory.
The development version deliberately rejects Profile V3 and its former
`[codex]` table without aliases, migration, or inferred defaults.
Workspace selection belongs to each CLI invocation, not the Profile.

The natural entry points also enable the authenticated `cloister_host` MCP
bridge by default. Codex receives transient `--config` values; Claude receives
a transient inline `--mcp-config` with an environment-backed authorization
header. Cloister does not add `--strict-mcp-config`, so the injected bridge does
not suppress the user's other Claude MCP sources. Neither path mutates
persistent agent configuration. `--no-host-bridge` provides an explicit
minimum-capability invocation.

`--dry-run` resolves and displays the selected workspace and state mount paths
without creating or changing the agent state directory.

## Security boundary

Shared state is shared across projects and may contain renewable authentication
tokens, configuration, history, and skills. It is a secret and it weakens
project-to-project state isolation by design. Cloister does not mount the host's
pre-existing `~/.codex` or `~/.claude`.

The workspace remains a live read-write bind mount by default. The agent can
modify or delete files in it.

The default host bridge is a stronger boundary crossing: `host.exec` can run
arbitrary commands with the macOS user's permissions. The runtime plan and
startup output must state this capability directly.
