# On-demand Host tools acceptance

Development verification on 2026-10-10: `make format` and `make verify` passed
all 229 tests and Clippy. The compiled CLI also parsed and validated the
on-demand example. These checks include authenticated loopback MCP transport
and temporary Host executables, but no logged-in Agent or live Guest acceptance.

On 2026-10-11, the development image also passed actual Guest-to-Host
connectivity and executable-launch checks with Apple container 1.5.0,
Codex 0.162.1, and Claude Code 2.1.296. These did not perform authenticated
model tasks.

Use the matching 0.4.0 CLI/image for release acceptance, or build a development
guest image from this branch. Published 0.3.1 does not contain the updated
canonical Host Skill.

```sh
make verify
cargo build
make image IMAGE_TAG=cloister:on-demand
```

Create a separate Profile from `examples/profile-on-demand.toml`, which selects
the exact 0.4.0 release image. For a local development build, explicitly change
its image reference to `cloister:on-demand`. Choose an explicit test workspace. Keep Profiles,
credentials, caches, audit logs, and transcripts out of Git. Initialization also
supports `cloister init --host-exec-mode on-demand --profile <new-profile>`;
select the development image in its existing image prompt.

Inspect `codex --profile <profile> --dry-run` and the equivalent Claude plan.
They must disclose on-demand mode, broad macOS-user authority, automatic Host
tool approval configuration, read-only software/root, and the selected workspace.
`--no-host-bridge` must remove the bridge and its MCP approval configuration.

With deliberately provisioned test accounts, record exact CLI, image, Apple
container and Agent versions. For both Agents verify:

1. Ordinary workspace reads, edits and searches use Guest tools.
2. A required host-only test executable absent from `host.exec.allow` is found
   through `host.find_command`, without running it during discovery.
3. The Agent explains why it needs Host execution and supplies `reason`, then
   uses the tool in the same conversation without registration, restart, or a
   Cloister-required execution prompt. Record any independent client policy
   prompts rather than claiming there were none.
4. Subsequent use reuses discovery. Long-running output uses bounded status
   waits, and cancellation stops the execution and descendants.
5. A missing command is reported without installation or an alternate escape.
6. An allowlist Profile continues to reject an unlisted command and request
   execution approval for permitted commands.

The repository tests exercise real authenticated loopback MCP transport and
test-controlled Host executables, but do not prove model routing, login,
current-client approval behavior, or Apple-container execution. Record those
separately; Agent guidance is not a Host sandbox.
