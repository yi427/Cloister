# Repository guidance

Cloister is a security-boundary product. Prefer explicit, inspectable behavior
over hidden defaults.

## Scope

- Keep the first product terminal-first and macOS/Apple-container focused.
- Treat the profile schema as a public contract and version it.
- Keep Apple `container` command construction separate from CLI presentation.
- Never silently mount host credential directories or privileged sockets.
- Never claim that a bind-mounted workspace is protected from writes made by
  the guest.
- Never equate a named container network with blocked internet access.

## Workflow

- Keep changes small and reviewable in Git.
- Do not commit local profiles, credentials, runtime state, or agent transcripts.
- Run `cargo fmt --check`, `cargo check`, `cargo test`, and
  `cargo clippy --all-targets -- -D warnings` before considering a Rust change
  complete.
- Add focused tests when profile parsing or command construction is introduced.
