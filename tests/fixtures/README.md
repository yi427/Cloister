# Test fixtures

Keep deterministic, repository-owned test inputs and expected outputs here.
Fixtures must not contain real credentials, agent state, host-specific absolute
paths, or data copied from a user's home directory.

Profile parser samples are organized by expected result:

```text
profiles/
  valid/
  invalid/
```

Valid fixtures represent the supported document shape; invalid fixtures target
one rejected condition each.
