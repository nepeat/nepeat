#!/usr/bin/env bash
set -eo pipefail
for attempt in {1..120}; do
    if /run/current-system/sw/bin/ip -4 -o address show | /run/current-system/sw/bin/grep -q ' inet 10\.60\.2\.190/'; then
        exit 0
    fi
    /run/current-system/sw/bin/sleep 1
done
echo "Timed out waiting for management address 10.60.2.190" >&2
exit 1
