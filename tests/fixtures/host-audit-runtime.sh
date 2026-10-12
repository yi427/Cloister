#!/bin/sh
set -eu
if [ "$1" = delete ]; then exit 0; fi
test "$1" = run
port=
for arg do
    case "$arg" in
        *http://host.container.internal:*/mcp*)
            address=${arg#*http://host.container.internal:}
            port=${address%%/mcp*}
            ;;
    esac
done
test -n "$port"
umask 077
printf '%s\n' "$CLOISTER_HOST_BRIDGE_TOKEN" > "$TEST_BRIDGE_TOKEN_FILE"
printf '%s\n' test-agent-active
client_status=0
"$TEST_CLOISTER_BINARY" host exec \
    --endpoint "http://127.0.0.1:$port/mcp" \
    --token-file "$TEST_BRIDGE_TOKEN_FILE" \
    --reason 'Test Host output and file auditing during an agent session' \
    host-tool -- "$TEST_HOST_EXIT_CODE" \
    > "$TEST_CLIENT_STDOUT" 2> "$TEST_CLIENT_STDERR" || client_status=$?
if [ "$TEST_HOST_EXIT_CODE" = 0 ]; then
    test "$client_status" = 0
else
    test "$client_status" = 1
fi
printf '%s\n' test-agent-finished
