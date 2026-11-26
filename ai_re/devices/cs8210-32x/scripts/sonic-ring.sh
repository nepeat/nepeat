#!/usr/bin/env bash
# Runs ON the CS8210 (root). Load through SONiC's own forwarding (CONFIG_DB / APPL_DB -> orchagent -> syncd ->
# sai-teralynx -> ASIC; no SDK shell): two counter-rotating rings over all three loops, the AS7772's
# sonic-ring.sh topology on this box's cables (1 <-> 2 and 31 <-> 32 DR4 optics, 3 <-> 17 DAC):
#   d1: Ethernet0 -> MPO -> Ethernet8 =Vlan100=> Ethernet16 -> DAC -> Ethernet128 =Vlan200=> Ethernet240 -> MPO ->
#       Ethernet248 =Vlan300=> Ethernet0 ...
#   d2: the reverse (Ethernet8 -> Ethernet0 =Vlan300=> Ethernet248 -> Ethernet240 =Vlan200=> Ethernet128 -> ...)
# Known unicast only (six static FDB entries through swssconfig); each source address enters each VLAN on one
# port only, so it is learnt once and never moves (no learn churn: the learn thread is this box's soft spot,
# learn-wedge.md). sai-teralynx keeps the CPU out of VLAN floods (SAI_TERALYNX_CPU_FLOOD off). IPv6 off on the
# netdevs, the kernel bridge ports without learning / flooding, so nothing joins the ring from the host.
#   sonic-ring.sh check | up | inject | sample | stop | down      (shared code: the AS7772's sonic-test-lib.sh)
# 'up' snapshots CONFIG_DB and the kernel state (lib), removes the loops' router interfaces (the Nix base's /31s
# and VRF bindings), makes the VLANs; 'down' restores the snapshot and compares the whole CONFIG_DB.
set -u
here=$(cd "$(dirname "$0")" && pwd)
lib=
for l in "${SONIC_TEST_LIB:-}" "$here/sonic-test-lib.sh" "$here/../../edge-core-as7772-32x/loadtest/sonic-test-lib.sh"; do
  if [ -n "$l" ] && [ -r "$l" ]; then lib=$l; break; fi
done
[ -n "$lib" ] || { echo "sonic-test-lib.sh not found (devices/edge-core-as7772-32x/loadtest/): copy it next to this script"; exit 2; }
export ST_RUN=${ST_RUN:-/run/cs8210-tests}
# shellcheck source=../../edge-core-as7772-32x/loadtest/sonic-test-lib.sh
. "$lib"
st_args "$@"
export SONIC_CLI_IFACE_MODE=default
# SONIC_RING_LOOPS=2: only 3 <-> 17 and 31 <-> 32 (while 1 <-> 2 is down): Ethernet16 -> DAC -> Ethernet128 =Vlan100=>
# Ethernet240 -> MPO -> Ethernet248 =Vlan200=> Ethernet16 ... and the reverse.
LOOPS=${SONIC_RING_LOOPS:-3}
case "$LOOPS" in
3)
  P4="Ethernet0 Ethernet8 Ethernet16 Ethernet128 Ethernet240 Ethernet248"   # the ring ports (the lib's name)
  M4="Vlan100:Ethernet8 Vlan100:Ethernet16 Vlan200:Ethernet128 Vlan200:Ethernet240 Vlan300:Ethernet248 Vlan300:Ethernet0"
  VLANS="Vlan100 Vlan200 Vlan300"
  PEERS="Ethernet0:Ethernet8 Ethernet8:Ethernet0 Ethernet16:Ethernet128 Ethernet128:Ethernet16 Ethernet240:Ethernet248 Ethernet248:Ethernet240"
  INJECT="Ethernet0 Ethernet8"
  FDB="Vlan100:d1:Ethernet16 Vlan200:d1:Ethernet240 Vlan300:d1:Ethernet0 Vlan300:d2:Ethernet248 Vlan200:d2:Ethernet128 Vlan100:d2:Ethernet8" ;;
2)
  P4="Ethernet16 Ethernet128 Ethernet240 Ethernet248"
  M4="Vlan100:Ethernet128 Vlan100:Ethernet240 Vlan200:Ethernet248 Vlan200:Ethernet16"
  VLANS="Vlan100 Vlan200"
  PEERS="Ethernet16:Ethernet128 Ethernet128:Ethernet16 Ethernet240:Ethernet248 Ethernet248:Ethernet240"
  INJECT="Ethernet16 Ethernet128"
  FDB="Vlan100:d1:Ethernet240 Vlan200:d1:Ethernet16 Vlan200:d2:Ethernet248 Vlan100:d2:Ethernet128" ;;
*) echo "SONIC_RING_LOOPS=$LOOPS: 2 or 3"; exit 2 ;;
esac
NPORTS=$(echo $P4 | wc -w) NFDB=$(echo $FDB | wc -w)
export NFDB
export FDB_RE='^FDB_TABLE:Vlan(100|200|300):02-00-00-00-00-d[12]$'
SWSSCONFIG=${SWSSCONFIG:-$(systemctl cat sonic-orchagent 2> /dev/null | grep -o '/nix/store/[^ /]*sonic-swss[^/ ]*' | head -n1)/bin/swssconfig}
# shellcheck disable=SC2034  # read by sonic-test-lib.sh
ST_PHASES=('^VLAN_MEMBER[|]Vlan(100|200|300)[|]' '^VLAN[|]Vlan(100|200|300)$' '^INTERFACE[|]Ethernet(0|8|16|128|240|248)$'
  '^INTERFACE[|]Ethernet(0|8|16|128|240|248)[|]')
SYSCTL_IFS="default Bridge $P4"
st_kernel_extra() {
  local i; for i in $SYSCTL_IFS; do echo "sysctl net.ipv6.conf.$i.disable_ipv6=$(sysctl -n "net.ipv6.conf.$i.disable_ipv6" 2> /dev/null)"; done
}
st_kernel_restore_extra() {
  local k v
  while IFS='=' read -r k v; do
    k=${k#sysctl }; [ "$(sysctl -n "$k" 2> /dev/null)" = "$v" ] || st_x sysctl -q -w "$k=$v"
  done < <(grep '^sysctl ' "$1")
}
appl_fdb() { st_r "$APPL" --scan --pattern '*FDB_TABLE:*' | LC_ALL=C sort; }
st_snapshot_extra() { appl_fdb > "$ST_DIR/appl-fdb-before.txt"; }
st_verify_extra() {
  if diff <(appl_fdb) "$ST_DIR/appl-fdb-before.txt" > /dev/null; then echo "== APPL_DB FDB keys as in the snapshot"
  else echo "== FAIL: APPL_DB FDB keys differ from the snapshot:"; diff <(appl_fdb) "$ST_DIR/appl-fdb-before.txt"; return 1; fi
}
members() {
  local m
  for m in $M4; do
    if [ "$1" = hset ]; then st_w "$CDB" hset "VLAN_MEMBER|${m%%:*}|${m#*:}" tagging_mode untagged
    else st_w "$CDB" del "VLAN_MEMBER|${m%%:*}|${m#*:}"; fi
  done
}
fdb_json() {   # $FDB entries VLAN:dN:PORT -> swssconfig JSON, OP $1
  local e sep=' ' v rest
  echo '['
  for e in $FDB; do
    v=${e%%:*} rest=${e#*:}
    printf '%s{"FDB_TABLE:%s:02-00-00-00-00-%s": {"port": "%s", "type": "static"}, "OP": "%s"}\n' "$sep" "$v" "${rest%%:*}" "${rest#*:}" "$1"
    sep=','
  done
  echo ']'
}
ring_check() {
  local k p v a peer host e
  ST_REFUSED=0
  [ -e "$ST_RUN/sonic-ring/configdb-before.tsv" ] && st_refuse "$ST_RUN/sonic-ring exists: the ring is up, or an 'up' did not finish ('down' first)"
  st_other_runs sonic-ring
  [ -x "$SWSSCONFIG" ] || st_refuse "swssconfig not found ($SWSSCONFIG)"
  for v in $VLANS; do st_has "$CDB" "VLAN|$v" && st_refuse "CONFIG_DB VLAN|$v exists"; done
  for k in $(st_r "$CDB" --scan --pattern 'FDB|*' | head -5); do st_refuse "CONFIG_DB static FDB entry $k exists"; done
  for k in $(appl_fdb | head -5); do st_refuse "APPL_DB FDB entry $k exists"; done
  for p in $P4; do for k in $(st_r "$CDB" --scan --pattern "VLAN_MEMBER|*|$p"); do st_refuse "CONFIG_DB $k exists"; done; done
  # oper state: exactly the ring ports up (anything else up would be a cable we do not know: house network)
  for k in $(st_r "$APPL" --scan --pattern 'PORT_TABLE:Ethernet*' | LC_ALL=C sort -V); do
    p=${k#PORT_TABLE:} v=$(st_r "$APPL" hget "$k" oper_status)
    case " $P4 " in
      *" $p "*) [ "$v" = up ] || st_refuse "APPL_DB $k oper_status=${v:-none}: a ring port is not up" ;;
      *) [ "$v" = up ] && st_refuse "APPL_DB $k oper_status=up: only the $NPORTS ring ports may be up" ;;
    esac
  done
  host=$(st_hget 'DEVICE_METADATA|localhost' hostname); host=${host:-$(hostname)}
  for k in $(st_r "$APPL" --scan --pattern 'LLDP_ENTRY_TABLE:*' | LC_ALL=C sort -V); do
    v=$(st_r "$APPL" hget "$k" lldp_rem_sys_name)
    [ "$v" = "$host" ] || st_refuse "APPL_DB $k: LLDP neighbour '$v' is not this switch ($host): house-network guard"
  done
  for e in $PEERS; do
    p=${e%%:*} peer=${e#*:} k="LLDP_ENTRY_TABLE:${e%%:*}"
    st_has "$APPL" "$k" || { st_refuse "APPL_DB $k missing: no LLDP neighbour on $p yet"; continue; }
    a=$(st_r "$APPL" hget "$k" lldp_rem_port_desc)
    [ "$a" = "$peer" ] || [ "$(st_r "$APPL" hget "$k" lldp_rem_port_id)" = "$(st_hget "PORT|$peer" alias)" ] ||
      st_refuse "APPL_DB $k: $p sees '$a', expected its loop peer $peer"
  done
  return 0
}

case "${ST_ARGS[0]:-}" in
check)
  ring_check
  if [ "$ST_REFUSED" = 0 ]; then echo "CHECK OK: the $NPORTS ring ports up and looped to themselves (LLDP), nothing else up, no leftovers"
  else echo "CHECK FAIL: $ST_REFUSED finding(s)"; exit 1; fi
  ;;
up)
  ring_check; st_refusals || exit 1
  st_begin sonic-ring arm
  for i in $SYSCTL_IFS; do st_x sysctl -q -w "net.ipv6.conf.$i.disable_ipv6=1"; done
  for p in $P4; do for k in $(st_r "$CDB" --scan --pattern "INTERFACE|$p|*"); do st_w "$CDB" del "$k"; done; done
  for p in $P4; do for k in $(st_r "$CDB" --scan --pattern "INTERFACE|$p|*"); do st_wait_applied "$k" absent || exit 1; done; done
  for p in $P4; do st_has "$CDB" "INTERFACE|$p" && st_w "$CDB" del "INTERFACE|$p"; done
  for p in $P4; do st_wait_applied "INTERFACE|$p" absent || exit 1; done
  for v in $VLANS; do st_w "$CDB" hset "VLAN|$v" vlanid "${v#Vlan}"; done
  for v in $VLANS; do st_wait_applied "VLAN|$v" present || exit 1; st_x sysctl -q -w "net.ipv6.conf.$v.disable_ipv6=1"; done
  members hset
  for m in $M4; do st_wait_applied "VLAN_MEMBER|${m%%:*}|${m#*:}" present || exit 1; done
  for p in $P4; do st_wait "$p in the kernel bridge" st_kernel_master "$p" Bridge || exit 1; done
  for p in $P4; do st_x bridge link set dev "$p" learning off flood off mcast_flood off bcast_flood off; done
  fdb_json SET > "$ST_DIR/fdb.json"; st_x "$SWSSCONFIG" "$ST_DIR/fdb.json"
  # shellcheck disable=SC2016
  st_wait "$NFDB static FDB entries in APPL_DB" eval '[ "$(appl_fdb | grep -c -E "$FDB_RE")" = "$NFDB" ]' || exit 1
  show vlan brief
  [ "$ST_DRY" = 1 ] && exit 0
  st_disarm; echo "== ring configured; 'down' puts everything back (snapshot $ST_DIR)"
  ;;
inject)  # 740 frames per ring (the AS7772 mix); sizes are frame lengths incl. FCS
  [ -e "$ST_RUN/sonic-ring/configdb-before.tsv" ] || { echo "REFUSED: the ring is not up"; exit 1; }
  # no python3 on the host PATH: the interpreter SONiC's own CLI runs on
  py=$(command -v python3) || {
    w=$(grep -o '/nix/store/[^"]*show-wrapped' "$(readlink -f "$(command -v show)")" | head -1)
    py=$(sed -n '1s/^#! *//p' "$w" | cut -d' ' -f1)
  }
  INJECT="$INJECT" "$py" - << 'EOF'
import os, socket
mix = [(64,300),(128,100),(256,100),(512,60),(1024,60),(1518,100),(9000,20)]
for dev, da, sa in zip(os.environ['INJECT'].split(), ('d1', 'd2'), ('a1', 'a2')):
    s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW); s.bind((dev, 0))
    hdr = bytes.fromhex('0200000000' + da + '0200000000' + sa + '88b5'); n = 0
    for ln, cnt in mix:
        f = hdr + bytes(ln - 4 - len(hdr))
        for i in range(cnt): s.send(f); n += 1
    print(dev, 'sent', n)
EOF
  ;;
sample)  # one line per source, all with the same timestamp
  t=$(date -u +%s)
  for i in $P4; do oid=$(st_r 2 hget COUNTERS_PORT_NAME_MAP "$i")
    echo "$t PORT $i $(st_r 2 hmget "COUNTERS:$oid" SAI_PORT_STAT_IF_IN_UCAST_PKTS SAI_PORT_STAT_IF_OUT_UCAST_PKTS SAI_PORT_STAT_IF_IN_OCTETS SAI_PORT_STAT_IF_OUT_OCTETS SAI_PORT_STAT_IF_IN_DISCARDS SAI_PORT_STAT_IF_IN_ERRORS SAI_PORT_STAT_IF_OUT_DISCARDS SAI_PORT_STAT_IF_OUT_ERRORS SAI_PORT_STAT_IF_IN_FEC_CORRECTABLE_FRAMES SAI_PORT_STAT_IF_IN_FEC_NOT_CORRECTABLE_FRAMES SAI_PORT_STAT_IF_IN_FEC_SYMBOL_ERRORS | tr '\n' ' ')"
  done
  echo "$t OPTICS $(grep -E '^switch_optic_(temperature|rx_power_watts)' /run/prometheus-textfile/cs8210-optics.prom | sed -E 's/switch_optic_//; s/\{port="([0-9]+)"(,lane="([0-9]+)")?\}/:\1\/\3/' | tr '\n' ' ')"
  echo "$t CREDO $(grep -v '^#' /run/prometheus-textfile/cs8210-credo.prom 2> /dev/null | sed -E 's/.*cage="([0-9]+)"\} /\1:/' | tr '\n' ' ')"
  echo "$t ASIC $(cat /run/asic/temperature 2> /dev/null) FAND failsafe=$(grep -E '^fand_zone_failsafe' /run/prometheus-textfile/fand.prom | awk '{s+=$2} END {print s}') duty=$(grep -E '^fand_output_duty_percent\{output="top1"' /run/prometheus-textfile/fand.prom | awk '{print $2}') $(grep -E '^fand_fan_rpm\{output="top1"' /run/prometheus-textfile/fand.prom | sed -E 's/.*rotor="([a-z]+)".* /rpm_top1_\1=/' | tr '\n' ' ') $(grep -E '^fand_zone_temperature_celsius' /run/prometheus-textfile/fand.prom | sed -E 's/.*zone="([a-z]+)"\} /zone_\1=/' | tr '\n' ' ')"
  echo "$t SYNCD rss_kb=$(ps -o rss= -C syncd | head -1) vmsize_kb=$(ps -o vsz= -C syncd | head -1) cpu=$(ps -o pcpu= -C syncd | head -1) load=$(cut -d' ' -f1-3 /proc/loadavg) mem_avail_kb=$(awk '/MemAvailable/ {print $2}' /proc/meminfo)"
  echo "$t WATCH $(grep -E '^(cs8210_ifcs_watchdog_(triggers_total|restarts_total|alarm)|sai_teralynx_ifcs_guard_[a-z_]*)' /run/prometheus-textfile/cs8210-ifcs.prom | sed -E 's/cs8210_ifcs_watchdog_//; s/sai_teralynx_ifcs_guard_/guard_/' | tr '\n' ' ') fpga_up=$(grep '^cs8210_optics_fpga_up' /run/prometheus-textfile/cs8210-optics.prom | awk '{print $2}') failed_units=$(systemctl --failed --no-legend | wc -l) oper_up=$(for p in $P4; do st_r "$APPL" hget "PORT_TABLE:$p" oper_status; done | grep -c '^up$')"
  ;;
stop)
  members del
  [ "$ST_DRY" = 1 ] || sleep 8
  show vlan brief
  ;;
down)
  if appl_fdb | grep -q -E "$FDB_RE"; then
    f=$(mktemp); fdb_json DEL > "$f"; st_x "$SWSSCONFIG" "$f"; rm -f "$f"
    # shellcheck disable=SC2016
    st_wait "static FDB entries gone from APPL_DB" eval '[ -z "$(appl_fdb | grep -E "$FDB_RE")" ]'
  fi
  st_cleanup sonic-ring
  ;;
*) sed -n '2,15p' "$0"; exit 2;;
esac
