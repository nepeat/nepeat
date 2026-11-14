#!/usr/bin/env bash
set -eo pipefail
cd /root/papermc
export MCRCON_HOST=10.60.2.190 MCRCON_PORT=25575
MCRCON_PASS=$(sed -n 's/^rcon.password=//p' server.properties)
export MCRCON_PASS
if [ -z "$MCRCON_PASS" ]; then
    echo "RCON password is not configured" >&2
    exit 1
fi
exec mcrcon "$@"
