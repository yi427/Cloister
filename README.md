# Cloister

Cloister is a terminal-first, privacy-oriented development environment for AI
coding agents. It will use Apple's `container` CLI to run Codex, Claude Code,
and development toolchains inside lightweight Linux virtual machines on Apple
silicon.

The project is currently at the environment-definition stage. The Rust binary
is only a placeholder; it does not create or manage containers yet.

## MVP boundary

The first useful version will:

- read a versioned TOML profile;
- start an ARM64 Linux environment through Apple `container`;
- apply CPU, memory, locale, timezone, user, and workspace settings;
- support a live bind-mounted workspace and an isolated copy workspace;
- keep each environment's agent state separate from host credentials;
- make network and writable-mount choices visible before launch;
- stop and remove environments without deleting the host project.

It will not initially provide a general-purpose container runtime, a GUI, or
transparent bidirectional file synchronization.

## Development baseline

- Host: Apple silicon Mac
- Runtime: Apple `container` 1.2 or newer
- Language: Rust 1.97.1, edition 2024
- Guest direction: Debian-family ARM64 image
- Agent runtime direction: Node.js LTS plus pinned Codex and Claude Code builds

The current profile shape is illustrated in
[`examples/profile.toml`](examples/profile.toml). Architectural and security
decisions are recorded in
[`docs/adr/0001-development-environment.md`](docs/adr/0001-development-environment.md).

## Project layout

```text
src/             Rust application code and module unit tests
tests/           Cross-module and CLI integration tests
tests/fixtures/  Deterministic test inputs and expected outputs
examples/        User-facing configuration examples
docs/adr/        Architecture decision records
```

## Development workflow

Enable the repository-managed Git hooks once after cloning:

```sh
make install-hooks
```

Format the working tree:

```sh
make format
```

Run the same verification used before each commit:

```sh
make verify
```

The pre-commit hook verifies the exact staged snapshot in a temporary directory.
It does not modify files or stage formatting changes automatically. A commit is
rejected if formatting, compilation, tests, or Clippy fail.
