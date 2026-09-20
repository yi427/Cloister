#!/bin/sh
# Deterministic read-only pfctl stand-in; rejects every unexpected command.
set -eu
case "$*" in
    '-n -v -f /etc/pf.conf')
        case "${TEST_PF_CONFIG:-ready}" in
            invalid) echo 'syntax error' >&2; exit 1 ;;
            disconnected) printf '%s\n' 'rdr-anchor "/*" all'; exit 0 ;;
        esac
        printf '%s\n' 'rdr-anchor "com.apple.container" all'
        if [ "${TEST_PF_CONFIG:-ready}" != missing-rule ]; then
            printf '%s\n' 'rdr inet from any to 203.0.113.113 -> 127.0.0.1'
        fi
        ;;
    *) echo 'unexpected pfctl mutation/query' >&2; exit 90 ;;
esac
