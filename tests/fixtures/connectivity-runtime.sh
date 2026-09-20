# Prepended to fake runtimes for check/init tests. Only the HTTP probe is handled.
# No external network or real container runtime is used.
if [ "$1" = run ]; then
    probe_url=
    for arg do
        case "$arg" in http://host.container.internal:*/cloister-check/*) probe_url=$arg ;; esac
    done
    if [ -n "$probe_url" ]; then
        if [ -n "${TEST_PROBE_RECORD:-}" ]; then printf '%s\n' "$*" >> "$TEST_PROBE_RECORD"; fi
        case "${TEST_PROBE_MODE:-ready}" in
            fail) exit 7 ;;
            no-contact) exit 0 ;;
            wrong-path) probe_url=${probe_url%/*}/wrong ;;
            stall) exec /bin/sleep 60 ;;
        esac
        # Simulate guest routing with a real, local HTTP request to this check's listener.
        exec /usr/bin/curl --disable --noproxy '*' --silent --fail --max-time 3 \
            "http://127.0.0.1:${probe_url#http://host.container.internal:}"
    fi
fi
if [ "$1:$2" = delete:--force ]; then
    case "$3" in
        cloister-check-*)
            if [ -n "${TEST_PROBE_RECORD:-}" ]; then printf '%s\n' "$*" >> "$TEST_PROBE_RECORD"; fi
            exit 0 ;;
    esac
fi
