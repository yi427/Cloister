#!/bin/sh
set -eu

mkdir -p "${HOME}" "${CARGO_HOME}" "${CODEX_HOME}" "${CLAUDE_CONFIG_DIR}"

exec "$@"
