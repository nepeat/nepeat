#!/usr/bin/env bash
# l2lag chip test helper (box side, run by sai-script exec steps): teamd LACP
# between PortChannel1 (Ethernet0, cage 1) and PortChannel2 (Ethernet120,
# cage 16) over the 400G loop. Copied to /run/l2lag/ on the box;
#   l2lag-teamd.sh up | wait-sync SECS | state | down
set -u
unset LD_LIBRARY_PATH   # sai-script's (SDK dir) must not reach teamd
export PATH=/run/current-system/sw/bin:$PATH   # the transient unit has no NixOS PATH
# LIBTEAM: libteam-sonic for aarch64 (ai_re/nix/packages/sonic libteam, nix copy to the box)
: "${LIBTEAM:=$(ls -d /nix/store/*-libteam-sonic-aarch64-unknown-linux-gnu-* 2>/dev/null | head -1)}"
T=${LIBTEAM:?no libteam-sonic in the store}/bin
D=/run/l2lag
pcs="PortChannel1:Ethernet0 PortChannel2:Ethernet120"   # no team hwaddr: a MAC change on the member netdev breaks teamd (chip 2026-11-16)
case $1 in
up)
  modprobe team && modprobe team_mode_loadbalance || exit 1
  # Both member netdevs carry the switch MAC: give PortChannel2 another system
  # id (two LACP ends on one box), on the netdev before teamd takes it.
  ip link set Ethernet120 down; ip link set Ethernet120 address 02:00:00:00:01:20 || exit 1
  for x in $pcs; do
    pc=${x%%:*}; port=${x#*:}
    cat > $D/$pc.conf <<EOF
{"device": "$pc",
 "runner": {"name": "lacp", "active": true, "fast_rate": true},
 "link_watch": {"name": "ethtool"}, "ports": {"$port": {}}}
EOF
    ip link set $port down   # the kernel refuses an up port as team port (EBUSY)
    # Foreground teamd in the background, team devices up only after both
    # started: with -d and an immediate "link up", teamd's port refresh raced
    # ("Failed to find enabled option", ports dropped; chip 2026-11-16).
    $T/teamd -f $D/$pc.conf -t $pc > $D/$pc-teamd.txt 2>&1 &
  done
  sleep 2
  for x in $pcs; do ip link set ${x%%:*} up; done
  echo "TEAMD up"
  ;;
wait-sync)
  for i in $(seq 1 "${2:-30}"); do
    ok=0
    for x in $pcs; do
      pc=${x%%:*}; port=${x#*:}
      s=$($T/teamdctl $pc state item get ports.$port.runner.selected 2>/dev/null)
      st=$($T/teamdctl $pc state item get ports.$port.runner.state 2>/dev/null)
      [ "$s" = true ] && [ "$st" = current ] && ok=$((ok + 1))
    done
    if [ $ok = 2 ]; then echo "TEAMD sync after ${i}s"; bash "$0" state; exit 0; fi
    sleep 1
  done
  echo "TEAMD no sync after ${2:-30}s"; bash "$0" state; exit 1
  ;;
state)
  for x in $pcs; do
    pc=${x%%:*}; port=${x#*:}
    echo "TEAMD $pc: link=$(cat /sys/class/net/$port/carrier 2>/dev/null) selected=$($T/teamdctl $pc state item get ports.$port.runner.selected) state=$($T/teamdctl $pc state item get ports.$port.runner.state) partner=$($T/teamdctl $pc state item get ports.$port.runner.partner_lacpdu.system 2>/dev/null) actor_state=$($T/teamdctl $pc state item get ports.$port.runner.actor_lacpdu.state 2>/dev/null) partner_state=$($T/teamdctl $pc state item get ports.$port.runner.partner_lacpdu.state 2>/dev/null)"
  done
  ;;
down)
  for x in $pcs; do
    pc=${x%%:*}; port=${x#*:}
    $T/teamd -k -t $pc 2>/dev/null
    sleep 0.5
    grep -m 5 -i -E "fail|error" $D/$pc-teamd.txt | sed "s/^/TEAMD $pc log: /"
    ip link del $pc 2>/dev/null
    ip link set $port up 2>/dev/null
  done
  echo "TEAMD down"
  ;;
esac
