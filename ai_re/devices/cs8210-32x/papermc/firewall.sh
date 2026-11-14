#!/usr/bin/env bash
set -euo pipefail
iptables=/run/current-system/sw/bin/iptables
rule=(-d 10.60.2.190 -p tcp --dport 25565 -m comment --comment cs8210-papermc -j ACCEPT)
case "${1:?open or close}" in
  open) "$iptables" -w -C INPUT "${rule[@]}" 2>/dev/null ||
        "$iptables" -w -I INPUT 1 "${rule[@]}" ;;
  close) if "$iptables" -w -C INPUT "${rule[@]}" 2>/dev/null; then
           "$iptables" -w -D INPUT "${rule[@]}"
         fi ;;
  *) exit 2 ;;
esac
