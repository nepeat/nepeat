#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
tty_options=()
if [[ ${OPENWRT_TTY:-0} == 1 ]]; then tty_options=(-tt); fi
exec ssh "${tty_options[@]}" -i "$root/work/management_key" \
    -o IdentitiesOnly=yes -o PasswordAuthentication=no \
    -o StrictHostKeyChecking=accept-new \
    -o UserKnownHostsFile="$root/work/known_hosts" -o ConnectTimeout=5 \
    "root@${OPENWRT_ADDRESS:-10.60.2.203}" "$@"
