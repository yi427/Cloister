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
