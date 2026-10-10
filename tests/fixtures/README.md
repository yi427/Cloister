# Test fixtures

Keep deterministic, repository-owned test inputs and expected outputs here.
Fixtures must not contain real credentials, agent state, host-specific absolute
paths, or data copied from a user's home directory.

Profile parser samples are organized by expected result:

```text
profiles/
  valid/
  invalid/
  preflight/
```

Valid fixtures represent the supported document shape; invalid fixtures target
one rejected parsing or static-validation condition each. Preflight fixtures
parse successfully but target a host-dependent failure.

`software-runtime.sh` simulates container installation and session lifecycles for
CLI tests. Paths, delays, and versions are supplied by temporary test directories.
It makes no network calls and must never be used as a production runtime.

`packet-filter/` contains synthetic, expanded PF rules and a fake `pfctl` for
readiness tests. The fake supports only read-only queries and does not inspect
or change the machine's firewall, DNS, or privileges.

`connectivity-runtime.sh` is prepended to check/init fake runtimes. It substitutes
a local HTTP request for the probe container, supports failure/interruption
scenarios, and records only test-controlled arguments in temporary directories.
