# ADR 0003: Natural agent entry point and shared state

- Status: Accepted for MVP
- Date: 2026-07-29

## Context

Requiring a project-specific Profile path and a complete container command for
every session makes the normal AI workflow feel like a container-runtime
wrapper. The common action should instead be running an agent in the current
project.

Agent login, configuration, sessions, and skills also need to survive an
ephemeral container. Mounting the host's existing `~/.codex` would expose state
that Cloister did not create or select.

## Decision

The high-level Codex workflow is:

```text
cd <project>
cloister codex
```

Cloister does not expose a general-purpose `run` command. Generic Apple
container planning and execution remain internal implementation details used by
agent-specific commands.

It uses the current directory as `/workspace` unless `--workspace` selects
another directory, reuses the configured OCI image, removes the container after
exit, and persists shared Codex state in a Cloister-managed directory:

```text
~/.local/share/cloister/agents/codex
```

That directory is mounted at `/cloister/agents/codex`, exposed to Codex through
`CODEX_HOME`, and restricted to owner-only host permissions. Cloister rejects a
symbolic link at the state-directory boundary.

Profile selection uses this order:

1. an explicit `--profile`;
2. `~/.config/cloister/profile.toml`.

There is no embedded fallback Profile. A missing default file is an explicit
configuration error.

`XDG_CONFIG_HOME` and `XDG_DATA_HOME` replace their respective home-relative
base directories when set. The Profile controls image, resources, guest
settings, explicit default networking, and whether Codex state is `shared` or
`isolated`.
Workspace selection belongs to each CLI invocation, not the Profile.

The natural entry point also enables the authenticated `cloister_host` MCP
bridge by default. Its configuration is injected for one Codex process and does
not mutate persistent `config.toml`. `--no-host-bridge` provides an explicit
minimum-capability invocation.

`--dry-run` resolves and displays the selected workspace and state mount paths
without creating or changing the agent state directory.

## Security boundary

Shared state is shared across projects and may contain renewable authentication
tokens, configuration, history, and skills. It is a secret and it weakens
project-to-project state isolation by design. Cloister does not mount the host's
pre-existing `~/.codex`.

The workspace remains a live read-write bind mount by default. The agent can
modify or delete files in it.

The default host bridge is a stronger boundary crossing: `host.exec` can run
arbitrary commands with the macOS user's permissions. The runtime plan and
startup output must state this capability directly.
