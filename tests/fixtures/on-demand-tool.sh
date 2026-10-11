#!/bin/sh
if [ -n "${CLOISTER_TEST_TOOL_MARKER:-}" ]; then
  printf '%s\n' invoked > "$CLOISTER_TEST_TOOL_MARKER"
fi
printf '%s\n' "$@"
