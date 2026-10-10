# Profile V7 Agent software acceptance

## Verified on 2026-09-20

The development branch was tested on an Apple silicon Mac using Apple container
1.4.1. The independently built test image was `cloister:agent-runtime-v2`.
All live checks used temporary Profiles, workspaces, software caches, isolated
Agent state, and no existing Agent credentials. No authentication or model task
was performed.

| Check | Result |
| --- | --- |
| Build base image without downloading either Agent | Passed |
| First-use Codex download and executable launch | Passed, codex-cli 0.155.1 |
| First-use Claude download and executable launch | Passed, Claude Code 2.1.278 |
| Two concurrent Codex launches with different workspaces | Passed |
| OS-allocated Host bridges for those launches | Different bound ports, both released on exit |
| Offline Claude launch from existing software cache | Passed |
| Explicit Codex update check with unchanged upstream version | Passed, existing installation reused |
| No codex/claude executable in unmounted base image | Passed |
| Software executable available from read-only session mount | Passed |
| Attempted write to software mount | Rejected by the actual guest mount |
| Codex isolated-state helper creation outside `/tmp` | Passed without the earlier temporary-directory warning |

The tested image manifest list was
`sha256:f2f618027219a63884074c317b597c8500e5b3243e025df3632fcda7f39d7ef7`.
The development tag is mutable; this digest identifies the image used for these
checks and is not a published release artifact.

Live testing found and fixed two issues that fake-runtime tests did not reveal:
npm rejects using the same file as both user and global configuration; and the
downloaded Codex version refuses to create helper binaries beneath `/tmp`.
The installer now uses distinct empty npm configuration files. Isolated Codex
state uses `/cloister/session/codex` in a per-container tmpfs. The root filesystem
remains read-only.

## Repeatable checks

Build the CLI and image, then create a separate V7 Profile selecting that image,
`agent.state = "isolated"`, and `host.exec.allow = []`. Use separate temporary
`XDG_DATA_HOME` and `XDG_STATE_HOME` directories outside the selected workspace.
Do not reuse a user's existing authentication state for smoke tests.

```sh
cargo build
make image IMAGE_TAG=cloister:agent-runtime-v2
```

Run each Agent with `--no-host-bridge -- --version` for first-use and cached
installation checks. Repeat with `--offline`, then `--update-agent`. Enable the
empty Host Exec policy and launch two `--offline -- --version` sessions from
different workspaces to check distinct printed bridge endpoints. These checks
need a terminal/PTY because the Agent container is interactive.

The deterministic suite additionally exercises install-lock contention,
interrupted installs/sessions, exact-version rollback, automatic update failure
fallback, explicit-update failure, unsafe cache paths, writable mount overlap,
per-session tokens, parser rejection, and command construction.
Image-switch regression tests also verify that each image retains its own offline
selection and automatic-update interval:

```sh
make format
make verify
```

## Host connectivity check

The readiness check now also verifies expanded PF configuration and performs an
IPv4 HTTP request from a temporary container to its own host loopback listener.
This passed on the same local image on 2026-09-20 without administrator access,
Agent credentials, or project mounts. The container was removed after the check.
Initial DNS resolution took about five seconds in live testing, so the probe
allows eight seconds for DNS/connect and ten seconds for the request, within a
twenty-second overall deadline. Deterministic tests cover failed requests,
false-success exits, mismatched probe paths, timeout, and signal cleanup.

## User confirmation on 2026-10-10

After the Apple container 1.5 PF-anchor recognition fix, the user reported that
the current development build tested successfully and accepted it. This is a
user-reported acceptance result; exact Agent versions and per-check output were
not recorded in this confirmation.

## Published 0.3.0 validation on 2026-10-10

The published
[release record](https://github.com/yi427/Cloister/releases/tag/v0.3.0)
documents checks against the exact release image, separately from the earlier
development-image checks above. The release commit is
`f883ea1901188dfad0bf9610e07321907c5b23f4`; the image digest is
`sha256:bf2528421b37aaf89863a76576b0a4375e8841c3549a99481d5cbe07733ff02d`.

The recorded checks include local `make verify` (212 tests and Clippy), GitHub
CI, anonymous Linux ARM64 image access, source installation, Homebrew source
installation/audit/test, base tool versions, Agent first-use and cached/offline
launches, exact-version selection, an explicit update check, concurrent Codex
launches with distinct bridge ports, and readiness including actual host
connectivity. Those launches installed Codex 0.162.1 and Claude Code 2.1.296.

No authenticated model tasks were performed in these release smoke checks.
These recorded versions do not certify later independently installed Agents.

## Coverage limits

Version output does not prove authenticated coding, skill selection, MCP
connection behavior, or tool approvals. The user's acceptance above does not
provide a per-check record for both Agents. For reproducible authenticated
acceptance, use deliberately provisioned test accounts to validate workspace
editing plus Host Exec discovery/execution/status/cancellation and record the
exact independently installed versions. No Profile migration or old-Cloister
compatibility is promised.
