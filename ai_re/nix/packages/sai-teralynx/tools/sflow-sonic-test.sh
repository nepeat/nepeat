#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# sflow-sonic-test.sh: SONiC sFlow end to end on the CS8210 loop, by hand,
# without a new generation (box side, root, ASIC lease held). Runtime only:
#   up   IPD_KO SAI HSFLOWD SFLOWTOOL PIM PROBE   swap in the patched ipd.ko
#        (legacy_abi=1 psample=1) and this libsai (LD_LIBRARY_PATH drop-in on
#        sonic-syncd), start SONiC, then sflowmgrd + port_index_mapper as
#        transient units (hsflowd is started by sflowmgrd through a "service"
#        shim), sflowtool on 127.0.0.1:6343
#   run  RATE COUNT    CONFIG_DB sFlow on Ethernet120 at 1 in RATE, COUNT frames
#        Ethernet0 -> Ethernet120 (sflow-probe also reads psample), report
#   down               undo everything: stock ipd, no drop-in, SONiC as booted
# Evidence goes to /run/sflow-test/.
set -euo pipefail
export PATH=/run/current-system/sw/bin:$PATH
ST=/run/sflow-test
die() { echo "sflow-sonic-test: $*" >&2; exit 3; }
db() { redis-cli -s /run/redis/redis.sock -n "$@"; }
cages() { q=$(cs8210-qsfp --enable 2>&1) || die "cs8210-qsfp failed"; [[ $q == *"present=00008001 "* ]] || die "cages: not exactly 1 and 16"; }
case ${1:-} in
up)
  ko=$2 sai=$3 hs=$4 st=$5 pim=$6 probe=$7
  cages
  mkdir -p $ST/bin
  printf '%s\n' "$hs" "$st" "$pim" "$probe" > $ST/paths
  systemctl stop sonic.target
  for _ in $(seq 30); do pgrep -x syncd >/dev/null || break; sleep 1; done
  ! pgrep -x syncd >/dev/null || die "syncd still running"
  sync
  rmmod ipd
  modprobe psample
  insmod "$ko" legacy_abi=1 psample=1
  [ "$(cat /sys/module/ipd/parameters/psample)" = Y ] || die "patched ipd not loaded"
  env=$(systemctl show sonic-syncd -p Environment --value)
  ld=$(tr ' ' '\n' <<<"$env" | sed -n 's/^LD_LIBRARY_PATH=//p')
  mkdir -p /run/systemd/system/sonic-syncd.service.d
  printf '[Service]\nEnvironment=LD_LIBRARY_PATH=%s/lib:%s\n' "$sai" "$ld" > /run/systemd/system/sonic-syncd.service.d/sflow-test.conf
  # sflowmgrd: "service hsflowd restart|stop"
  cat > $ST/bin/service <<EOF
#!/bin/sh
case "\$1 \$2" in
  "hsflowd restart"|"hsflowd start") systemctl stop sflow-hsflowd 2>/dev/null; exec systemd-run --unit=sflow-hsflowd -p RuntimeMaxSec=3600 $hs/bin/hsflowd -d -P -f $hs/etc/hsflowd.conf -l $hs/lib/hsflowd/modules -p $ST/hsflowd.pid -o $ST/hsflowd.auto ;;
  "hsflowd stop") exec systemctl stop sflow-hsflowd ;;
esac
exit 1
EOF
  chmod +x $ST/bin/service
  systemctl daemon-reload
  systemctl start sonic.target
  for _ in $(seq 120); do systemctl is-active -q sonic-orchagent && break; sleep 1; done
  swss=$(systemctl show sonic-orchagent -p ExecStart --value | grep -o '/nix/store/[^ ]*-sonic-swss-[^/ ]*' | head -1)
  sleep 20
  systemd-run --unit=sflow-sflowmgrd -p RuntimeMaxSec=3600 --setenv=PATH=$ST/bin:/run/current-system/sw/bin $swss/bin/sflowmgrd
  systemd-run --unit=sflow-pim -p RuntimeMaxSec=3600 $pim/bin/port_index_mapper
  systemd-run --unit=sflow-sflowtool -p RuntimeMaxSec=3600 -p StandardOutput=file:$ST/sflowtool.txt $st/bin/sflowtool -p 6343 -l
  db 4 hset 'FEATURE|sflow' state enabled has_global_scope True has_per_asic_scope False auto_restart enabled >/dev/null
  echo "up: sai $sai, ipd psample=$(cat /sys/module/ipd/parameters/psample)"
  ;;
run)
  rate=$2 n=$3
  probe=$(sed -n 4p $ST/paths)
  db 4 hset 'SFLOW_COLLECTOR|c1' collector_ip 127.0.0.1 collector_port 6343 collector_vrf default >/dev/null
  db 4 hset 'SFLOW_SESSION|Ethernet120' admin_state up sample_rate "$rate" >/dev/null
  db 4 hset 'SFLOW|global' admin_state up polling_interval 20 >/dev/null
  sleep 15
  echo "== APPL_DB SFLOW_SESSION_TABLE:Ethernet120: $(db 0 hgetall SFLOW_SESSION_TABLE:Ethernet120 | tr '\n' ' ')"
  echo "== ASIC_DB SAMPLEPACKET: $(db 1 keys 'ASIC_STATE:SAI_OBJECT_TYPE_SAMPLEPACKET:*' | while read k; do db 1 hgetall "$k" | tr '\n' ' '; done)"
  echo "== ASIC_DB GENETLINK hostif: $(db 1 keys 'ASIC_STATE:SAI_OBJECT_TYPE_HOSTIF:*' | while read k; do db 1 hgetall "$k" | tr '\n' ' ' | grep -o 'SAI_HOSTIF_TYPE_GENETLINK.*' ; done)"
  echo "== hsflowd: $(systemctl is-active sflow-hsflowd)"
  echo "== ifindex Ethernet120=$(cat /sys/class/net/Ethernet120/ifindex), PORT_INDEX_TABLE: $(db 6 hgetall 'PORT_INDEX_TABLE|Ethernet120' | tr '\n' ' ')"
  : > $ST/sflowtool.txt
  "$probe/bin/sflow-probe" psample Ethernet0 1 "$n" 20000 256 02:99:99:99:99:99 10.99.9.1 10.99.9.2
  sleep 25
  echo "== sflowtool: $(grep -c '^FLOW' $ST/sflowtool.txt) FLOW lines, $(grep -c '^CNTR' $ST/sflowtool.txt) CNTR lines"
  grep '^FLOW' $ST/sflowtool.txt | head -3
  grep '^FLOW' $ST/sflowtool.txt | awk -F, '{print "agent="$2" in="$3" out="$4" rate="$NF}' | sort | uniq -c | head
  ;;
down)
  db 4 hset 'SFLOW|global' admin_state down >/dev/null || true
  sleep 3
  db 4 del 'SFLOW|global' 'SFLOW_COLLECTOR|c1' 'SFLOW_SESSION|Ethernet120' 'FEATURE|sflow' >/dev/null || true
  for u in sflow-hsflowd sflow-sflowmgrd sflow-pim sflow-sflowtool; do systemctl stop $u 2>/dev/null || true; done
  systemctl stop sonic.target
  for _ in $(seq 30); do pgrep -x syncd >/dev/null || break; sleep 1; done
  rm -rf /run/systemd/system/sonic-syncd.service.d/sflow-test.conf
  rmdir /run/systemd/system/sonic-syncd.service.d 2>/dev/null || true
  systemctl daemon-reload
  sync
  rmmod ipd
  modprobe ipd
  [ "$(cat /sys/module/ipd/parameters/legacy_abi)" = Y ] || die "stock ipd not back"
  [ ! -e /sys/module/ipd/parameters/psample ] || die "still the patched ipd"
  systemctl start sonic.target
  echo "down: stock ipd, SONiC as booted"
  ;;
*) die "up|run|down" ;;
esac
