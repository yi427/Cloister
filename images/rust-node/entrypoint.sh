#!/bin/sh
set -eu

mkdir -p "${HOME}" "${CARGO_HOME}" "${CODEX_HOME}" "${CLAUDE_CONFIG_DIR}"

if [ -n "${CLOISTER_HOST_BRIDGE_TOKEN:-}" ]; then
    canonical_skill=/usr/local/share/cloister/skills/host-exec
    codex_skill_root="${HOME}/.agents/skills"
    codex_skill="${codex_skill_root}/host-exec"

    if [ ! -f "${canonical_skill}/SKILL.md" ]; then
        echo "Cloister Host Skill is missing from the image" >&2
        exit 1
    fi
    if [ -e "${codex_skill}" ] || [ -L "${codex_skill}" ]; then
        echo "refusing to overwrite existing Codex Skill at ${codex_skill}" >&2
        exit 1
    fi

    mkdir -p "${codex_skill_root}"
    ln -s "${canonical_skill}" "${codex_skill}"
fi

exec "$@"
