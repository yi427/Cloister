#!/bin/sh
# Runs only in a dedicated installer container. No project, credentials or bridge.
set -eu
case "$1" in
    codex) package=@openai/codex ;;
    claude) package=@anthropic-ai/claude-code ;;
    *) echo 'unsupported agent' >&2; exit 1 ;;
esac
export npm_config_registry=https://registry.npmjs.org
export npm_config_cache=/tmp/npm-cache
export npm_config_userconfig=/tmp/cloister-user.npmrc
export npm_config_globalconfig=/tmp/cloister-global.npmrc
export npm_config_update_notifier=false
export npm_config_fetch_retries=1
export npm_config_fetch_timeout=30000
mkdir -p "$HOME" "$CODEX_HOME"
# npm rejects loading the same file as both its global and user configuration.
: > "$npm_config_userconfig"
: > "$npm_config_globalconfig"
cd /tmp
# Resolve first; do not reinstall when the currently selected version is latest.
version=$(npm view "$package@$2" version)
if [ "$version" = "${3:-}" ]; then
    printf '%s\n' "$version" > /cloister/install/version
    exit 0
fi
npm install --prefix /cloister/install --no-audit --no-fund "$package@$version"
/cloister/install/node_modules/.bin/"$1" --version
node -e 'const fs=require("fs"); const p=JSON.parse(fs.readFileSync("/cloister/install/node_modules/"+process.argv[1]+"/package.json")); if(p.version!==process.argv[2])process.exit(1); fs.writeFileSync("/cloister/install/version",p.version+"\n")' "$package" "$version"
