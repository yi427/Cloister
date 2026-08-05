# ADR 0005: Explicit inherited proxy for guest connectivity

- Status: Accepted and implemented
- Date: 2026-08-05
- Updates: ADR 0001, ADR 0003, and Profile V6

## Context

Apple `container` default networking has provided outbound connectivity on the
development machine, but it is not a promise that every destination is always
reachable. In particular, a macOS proxy listening on loopback cannot be reached
from the guest as `127.0.0.1`: that address refers to the guest itself.

Cloister previously relied only on the default container route. When direct
guest access to model endpoints stopped working while the host proxy still
worked, the product needed an explicit and inspectable way to forward that
proxy. Cloister must not silently copy proxy settings into every guest, print a
credential-bearing URL, or imply that a proxy is an egress sandbox.

## Decision

Profile V6 requires a guest proxy policy beside the existing network mode:

```toml
[network]
mode = "default"
proxy = "disabled" # or "inherit"
```

`disabled` ignores proxy variables in Cloister's host environment. `inherit`
resolves one proxy at launch from the first non-empty variable in this order:

1. `HTTPS_PROXY`
2. `https_proxy`
3. `ALL_PROXY`
4. `all_proxy`
5. `HTTP_PROXY`
6. `http_proxy`

The selected value must be valid UTF-8 and an absolute `http` or `https` URL
with a host. An explicit `inherit` policy fails closed when no supported value
exists or validation fails. The Profile stores only the policy, never the
resolved value.

If the selected URL uses `localhost` or a loopback IPv4 or IPv6 address,
Cloister rewrites only its host to `host.container.internal`. The port, scheme,
path, query, and user information remain part of the URL. Apple `container`
must therefore have the localhost DNS mapping required by the Host MCP bridge.

Cloister exposes the one resolved URL under all six conventional upper- and
lowercase HTTP, HTTPS, and ALL proxy names. It merges existing `NO_PROXY` and
`no_proxy` entries, removes exact duplicates case-insensitively, and ensures
that these destinations bypass the proxy:

- `host.container.internal`
- `localhost`
- `127.0.0.1`
- `::1`

This normalization gives tools with different variable conventions the same
connectivity and keeps the authenticated Host MCP bridge on its direct route.

## Secret handling and command construction

Proxy URLs may contain usernames, passwords, tokens, or machine-local routing
details. Runtime command construction therefore adds only `--env NAME` to the
Apple `container run` argument vector. The resolved URL and merged no-proxy
list are set on the host `container` process as secret environment values;
Apple `container` reads those values by name when creating the guest.

Runtime plans, `--dry-run`, `check`, errors, and debug formatting may report the
source variable and whether loopback rewriting occurred. They must not include
the selected URL or no-proxy value. The Profile and agent configuration files
must not persist either value.

These controls prevent accidental disclosure by Cloister. They do not hide the
values from the guest. The selected agent and any guest process with access to
its environment can read the forwarded variables, including embedded proxy
credentials. Users who do not accept that exposure must select `disabled`.

## CLI behavior

`init` examines the current environment after collecting the basic guest
settings. When it finds a supported proxy, it identifies only the source
variable and asks whether to inherit it, defaulting to yes. It writes only
`proxy = "inherit"`. When no supported variable exists, it writes
`proxy = "disabled"`. An invalid detected variable is reported without its
value; the user can explicitly continue with inheritance disabled or cancel
initialization.

`check` resolves and validates the selected policy using one environment
snapshot. It reports disabled, missing, invalid, remote, or loopback-rewritten
state without displaying values. It deliberately does not contact a proxy or a
third-party endpoint: reachability is destination-specific and a read-only
configuration check must not create hidden network traffic.

Natural Codex and Claude launches use the same resolved snapshot for the
inspectable runtime plan and the actual `container` process. A Profile/runtime
mismatch is rejected instead of silently enabling or dropping the proxy.

## Security boundary

This feature restores connectivity through a user-selected host proxy. It is
not a network allowlist, content filter, anonymity boundary, or guarantee that
all traffic uses the proxy. Guest software may ignore these environment
variables, and the Profile still selects Apple `container`'s default network.

The existing claim remains unchanged: a named or default container network
must never be equated with blocked internet access. Strong egress enforcement
requires a separately designed, non-agent-controlled boundary and bypass tests.

## Alternatives considered

### Continue relying on default guest routing

This is simpler but does not address destinations that are reachable only
through the host proxy and makes failures dependent on changing host/runtime
routing behavior.

### Always inherit host proxy variables

This would be convenient but would silently expose proxy details and possible
credentials to every guest. It also makes runtime behavior depend on a hidden
default rather than the versioned Profile contract.

### Store the proxy URL in the Profile

This would make launches reproducible, but it would put a likely secret in a
long-lived configuration file that users may commit, copy, or inspect. The
policy remains versioned while the value stays ephemeral.

### Pass `--env NAME=value`

This would place secrets in an inspectable command line and dry-run output.
Name-only forwarding keeps values out of the argument vector.

## Acceptance criteria

- Profile V6 requires and validates `proxy = "disabled" | "inherit"` and
  rejects Profile V5 without migration.
- Disabled policy does not inspect malformed ambient proxy values.
- Inherit policy implements the documented precedence and fails when missing.
- Only HTTP and HTTPS proxy URLs are accepted.
- Loopback hosts are rewritten without exposing URL contents in errors or
  `Debug` output.
- Required direct destinations are merged into both no-proxy spellings.
- Apple `container` command arguments contain environment names but no values;
  values exist only in the spawned process environment.
- Codex and Claude use the same policy resolution path.
- `init`, `check`, and runtime-plan behavior are covered by public CLI tests.
- A real Apple-container probe confirms that a host loopback proxy is reachable
  through `host.container.internal`.
- `make verify` and the staged-snapshot pre-commit hook pass.

## Consequences

Users behind a host HTTP proxy can opt into predictable guest connectivity
without persisting its URL. Profile V6 is intentionally incompatible with V5,
so existing development Profiles must be recreated or edited explicitly. The
extra policy and diagnostics make connectivity more inspectable, while strict
redaction and explicit security text keep the resulting exposure honest.
