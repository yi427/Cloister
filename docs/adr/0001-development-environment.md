# ADR 0001: Development environment baseline

- Status: Accepted for MVP
- Date: 2026-07-29

## Context

Cloister will provide privacy-oriented development environments for AI coding
agents. The first client is a Rust terminal application on Apple silicon Macs.
Apple's `container` CLI is the runtime candidate because each Linux container is
run in a lightweight virtual machine while retaining OCI image compatibility.

The product must distinguish host isolation, workspace integrity, credential
isolation, network policy, and model-provider data handling. Calling all five
"privacy" would hide important differences.

## Verified host baseline

The initial development machine was checked with:

- macOS 27.0 on ARM64;
- Git 2.54.0;
- Rust and Cargo 1.97.1;
- Apple `container` CLI and API server 1.2.0.

A disposable Alpine ARM64 probe verified:

- the `container` system starts successfully;
- CPU and memory limits are accepted;
- a root read-only filesystem with a writable `tmpfs` starts;
- locale and timezone environment variables reach the guest;
- `/Volumes/Home/project/Cloister` mounts directly through virtiofs;
- a read-only workspace mount rejects guest writes;
- the default network provides outbound internet access.

This verifies an MVP compatibility slice, not every filesystem, image, network
policy, or long-running workload.

## Decision

### Runtime and guest

- Target Apple `container` 1.2 or newer on Apple silicon.
- Use native `linux/arm64`; do not make Rosetta or `linux/amd64` the default.
- Build a Debian-family guest image. It offers the glibc and package
  compatibility expected by common development and agent tools.
- Pin the base image by digest and pin agent CLI versions in reproducible image
  builds.
- Run the interactive agent as an unprivileged guest user.
- Make the guest root filesystem read-only where practical and provide explicit
  writable mounts for temporary files, workspace data, and isolated agent state.

### Rust project

- Use Rust edition 2024 and pin the development toolchain in
  `rust-toolchain.toml`.
- Begin as one binary crate. Split crates only after profile, policy, and runtime
  boundaries have real implementations.
- Keep profile parsing and validation independent from Apple `container` command
  construction so both can be tested without starting virtual machines.

### Profiles

Profiles are versioned TOML documents. The current schema covers:

- image reference and architecture;
- CPU and memory limits;
- guest user, locale, and timezone;
- network policy;
- agent state policy; and
- explicit host-command policy.

ADR 0003 later moved workspace selection out of the Profile. The CLI selects a
host directory for each invocation and mounts it at `/workspace`.

Unknown schema versions and unknown security-sensitive values must fail closed.
The Profile schema is a public contract. Breaking changes require a new schema
version even while the product is still under development.

### Workspace modes

`bind` mode directly mounts a selected host directory. Host editors and guest
agents see changes immediately. This is the simplest development experience,
but the guest can modify or delete every writable mounted file. Git is the
recovery and review layer, not an isolation layer.

`copy` mode will import a snapshot into Cloister-managed storage. Guest changes
remain isolated until the user explicitly reviews and exports them. Cloister
must not automatically copy an older guest snapshot over the host project when
the environment exits.

Transparent two-way synchronization is deferred until conflict, deletion,
metadata, symlink, and crash-recovery semantics can be specified.

### Agent credentials and state

Cloister will not mount the host's home directory, SSH agent socket,
`~/.ssh`, `~/.aws`, `~/.codex`, or `~/.claude` by default.

ADR 0003 later allows an explicit shared agent state policy. Interactive
device-code or browser authentication and secrets passed through standard input
are preferred over command-line arguments. Persistent agent state must be
stored separately from the project and must be removable without deleting the
project.

Codex file-backed authentication may contain renewable access tokens. Such
state is a secret, not a normal configuration file.

### Network

Apple `container` creates a default network with outbound access. Separate named
networks isolate groups of containers from each other; they do not, by
themselves, provide an internet denylist or allowlist.

The MVP may expose `default` networking while reporting it clearly. A
`restricted` policy is not complete until Cloister enforces egress with a
non-agent-controlled firewall or proxy and tests bypass attempts. Authentication
endpoints, model APIs, package registries, Git remotes, telemetry, and MCP
servers must be modeled separately.

## Threat boundary

Cloister is intended to reduce damage from commands executed by an AI agent and
to minimize incidental host credential exposure. It does not promise:

- protection for files deliberately mounted read-write;
- secrecy from the selected AI model provider when source is sent to it;
- safe execution of malicious kernel or virtualization exploits;
- trustworthy third-party images, packages, plugins, skills, or MCP servers;
- network privacy while unrestricted egress is enabled.

These limits must remain visible in CLI prompts and documentation.

## Consequences

The live bind-mount workflow can ship early because it already works on the
development machine. Stronger workspace integrity and egress control remain
separate milestones instead of being implied by the VM boundary.

Profile parsing, validation, command planning, and process execution remain
separate layers. The exact `container run` command can be inspected with
`--dry-run`, while real execution consumes the same argument vector without
shell parsing.
