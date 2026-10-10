#!/bin/sh
# Deterministic fake Apple runtime. All paths are provided by temporary tests.
set -eu
if [ "$1" = delete ]; then
    printf '%s\n' "$*" >> "$TEST_EVENTS"
    exit 0
fi
installer=false
staging=
name=
args="$*"
while [ "$#" -gt 0 ]; do
    case "$1" in
        --entrypoint) installer=true; shift 2 ;;
        --name) name=$2; shift 2 ;;
        --mount)
            case "$2" in
                *,target=/cloister/install) staging=${2#type=bind,source=}; staging=${staging%,target=/cloister/install} ;;
            esac
            shift 2 ;;
        --) shift; break ;;
        *) shift ;;
    esac
done
image=$1
agent=$2
if "$installer"; then
    [ -n "$staging" ]
    [ -z "${CLOISTER_HOST_BRIDGE_TOKEN:-}" ]
    printf 'INSTALL %s\n' "$args" >> "$TEST_EVENTS"
    /bin/sleep "${TEST_INSTALL_DELAY:-0}"
    [ "${TEST_INSTALL_FAIL:-0}" = 0 ] || { echo 'secret-proxy-password' >&2; exit 17; }
    version=$3
    if [ "$version" = latest ]; then version=${TEST_AGENT_VERSION:-1.2.3}; fi
    /bin/mkdir -p "$staging/node_modules/.bin"
    printf '#!/bin/sh\nexit 0\n' > "$staging/node_modules/.bin/$agent"
    /bin/chmod 755 "$staging/node_modules/.bin/$agent"
    printf '%s\n' "$version" > "$staging/version"
    exit 0
fi
printf 'SESSION %s\n' "$args" >> "$TEST_EVENTS"
printf '%s' "${CLOISTER_HOST_BRIDGE_TOKEN:-}" > "$TEST_RECORDS/$name.token"
/bin/sleep "${TEST_SESSION_DELAY:-0}"
exit 0
