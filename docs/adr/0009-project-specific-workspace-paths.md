# ADR 0009: Project-specific guest workspace paths

- Status: Accepted
- Date: 2026-10-10

## Context

Every selected Host project previously appeared at Guest `/workspace`. With
shared Agent state, sessions from unrelated projects recorded the same working
directory, so an agent's project-filtered resume picker could mix their histories.

## Decision

Mount the selected canonical Host path under Guest `/workspace` and use that
exact mount target as the container working directory. For example:

```text
/Volumes/Projects/app -> /workspace/Volumes/Projects/app
```

Use the complete canonical path, rather than only the directory name, so
namesake projects at different locations remain distinct. Canonicalization
keeps repeated launches and symbolic-link aliases of the same project stable.
The same rule applies to Codex, Claude, and explicit `--workspace` selection.
Only the selected Host directory is mounted; its ancestors are not exposed.

Agent credentials, configuration, and history retain the existing shared or
isolated state policy. The Profile schema is unchanged because no Profile
fields are added or reinterpreted. Project filtering remains an Agent behavior,
not an access restriction on shared history.

## Consequences

Moving a project changes its Guest working directory. Old sessions recorded at
`/workspace` stay untouched because their original Host project cannot be
reliably inferred. Codex users can find them through `resume --all`, which also
explicitly disables project filtering for newer sessions.

The workspace remains a live read-write bind mount. Guest writes and deletions
affect the selected Host project. Host Exec keeps its canonical Host working
directory and workspace-relative arguments.
