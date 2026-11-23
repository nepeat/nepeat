#!/usr/bin/env bash
# Runs ON the AS7772 (root, login shell). Load through SONiC's own forwarding path on the SAI in use (CONFIG_DB /
# APPL_DB -> orchagent -> syncd -> libsai -> ASIC; no SDK shell writes), MACsec left on as SONiC runs it.
# Two counter-rotating rings over both DACs, the same topology as ring.sh (SDK shell, SONiC stopped):
#   d1: Ethernet0 -> cable -> Ethernet4 =Vlan200=> Ethernet124 -> cable -> Ethernet120 =Vlan100=> Ethernet0 ...
#   d2: Ethernet4 -> cable -> Ethernet0 =Vlan100=> Ethernet120 -> cable -> Ethernet124 =Vlan200=> Ethernet4 ...
# Known unicast only: four static FDB entries (APPL_DB FDB_TABLE through swssconfig, as SONiC loads static
# config). Not a flood ring: our SAI makes the CPU a tagged member of every VLAN ("floods reach KNET"), so a
# flooded ring frame would be copied to the CPU at every hop. Guards against frames joining the ring from the
# host: IPv6 off on the four netdevs, Bridge and the new Vlan netdevs (neigh_discovery is a "copy" trap: an ND
# multicast would circulate and be copied), and no learning / flooding on the kernel bridge's ports (the kernel
# has no bridge netfilter) so a CPU copy cannot be bridged back out. The CPU injects 740 frames per ring (ring.sh's
# mix, ethertype 0x88b5) through the port netdevs (KNET, directed to the port).
#   sonic-ring.sh check | up | inject | sample | macsec | stop | down      (check, up, stop, down: also --dry-run)
# check (reads only; 'up' runs it first and refuses on any finding, naming the key): the four DAC ports oper up and
# no other port; LLDP on each of them sees this switch on the expected peer port, and no port sees another system
# (house-network guard); MKA up on the ports with a MACsec profile; no INTERFACE vrf_name / address on the four
# ports, no Vlan100/200, no VLAN members of the four, no static FDB entry; no unfinished run of this or of a
# traffic test (/run/as7772-tests). up: snapshot of CONFIG_DB, APPL_DB FDB keys, sysctls and kernel state, then
# the ring config (a failure half-way restores at once). down: FDB entries removed, CONFIG_DB keys, sysctls and
# kernel state put back, then the whole CONFIG_DB compared with the snapshot (a difference fails, loudly); without
# a snapshot (an 'up' of the older version) down restores the clean state it knows. Shared code:
# sonic-test-lib.sh (beside this script). The checks and the restore are not yet run on the switch.
set -u
here=$(cd "$(dirname "$0")" && pwd)
lib=
for l in "${SONIC_TEST_LIB:-}" "$here/loadtest/sonic-test-lib.sh" "$here/sonic-test-lib.sh"; do
  if [ -n "$l" ] && [ -r "$l" ]; then lib=$l; break; fi
done
[ -n "$lib" ] || { echo "sonic-test-lib.sh not found: copy loadtest/sonic-test-lib.sh next to this script"; exit 2; }
# shellcheck source=loadtest/sonic-test-lib.sh
. "$lib"
st_args "$@"
export SONIC_CLI_IFACE_MODE=default
M4="Vlan100:Ethernet0 Vlan100:Ethernet120 Vlan200:Ethernet4 Vlan200:Ethernet124"
PEERS="Ethernet0:Ethernet4 Ethernet4:Ethernet0 Ethernet120:Ethernet124 Ethernet124:Ethernet120"
D() { openbcm-diag "$@"; }
SWSSCONFIG=${SWSSCONFIG:-$(systemctl cat sonic-orchagent 2> /dev/null | grep -o '/nix/store/[^ /]*sonic-swss[^/ ]*' | head -n1)/bin/swssconfig}
ST_PHASES=('^VLAN_MEMBER[|]Vlan(100|200)[|]' '^VLAN[|]Vlan(100|200)$' '^INTERFACE[|]Ethernet(0|4|120|124)([|]|$)')
SYSCTL_IFS="default Bridge $P4"
st_kernel_extra() {
  local i; for i in $SYSCTL_IFS; do echo "sysctl net.ipv6.conf.$i.disable_ipv6=$(sysctl -n "net.ipv6.conf.$i.disable_ipv6" 2> /dev/null)"; done
}
st_kernel_restore_extra() {   # the sysctls 'up' set (the Vlan netdevs' go with the netdevs)
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
# the clean state for 'down' without a snapshot: no Vlan100/200 nor their members, the far ends' INTERFACE entries
# with exactly their mac_addr, the IPv6 sysctls 0 (boot values), the four ports outside the kernel bridge
st_synth_configdb() {
  awk -F'\t' -v m4="$FAR_MAC_Ethernet4" -v m124="$FAR_MAC_Ethernet124" '
    $1 ~ /^VLAN[|]Vlan(100|200)$/ || $1 ~ /^VLAN_MEMBER[|]Vlan(100|200)[|]/ || $1 ~ /^INTERFACE[|]Ethernet(4|124)$/ { next }
    { print }
    END { print "INTERFACE|Ethernet4\tmac_addr\t" m4; print "INTERFACE|Ethernet124\tmac_addr\t" m124 }' | LC_ALL=C sort
}
st_synth_kernel() { sed -E -e 's/^(port [^ ]+) master=Bridge /\1 master= /' -e '/^vlan /s/\bVlan(100|200) //g' -e 's/^(sysctl [^=]+)=.*/\1=0/'; }
st_synth_extra() { : > "$ST_DIR/appl-fdb-before.txt"; }

members() {   # hset|del
  local m
  for m in $M4; do
    if [ "$1" = hset ]; then st_w "$CDB" hset "VLAN_MEMBER|${m%%:*}|${m#*:}" tagging_mode untagged
    else st_w "$CDB" del "VLAN_MEMBER|${m%%:*}|${m#*:}"; fi
  done
}
fdb_json() {   # SET|DEL: the four static entries
  local op=$1
  cat << EOF
[
 {"FDB_TABLE:Vlan200:02-00-00-00-00-d1": {"port": "Ethernet124", "type": "static"}, "OP": "$op"},
 {"FDB_TABLE:Vlan100:02-00-00-00-00-d1": {"port": "Ethernet0", "type": "static"}, "OP": "$op"},
 {"FDB_TABLE:Vlan100:02-00-00-00-00-d2": {"port": "Ethernet120", "type": "static"}, "OP": "$op"},
 {"FDB_TABLE:Vlan200:02-00-00-00-00-d2": {"port": "Ethernet4", "type": "static"}, "OP": "$op"}
]
EOF
}
ring_check() {
  local k p v a peer host e want
  ST_REFUSED=0
  [ -e "$ST_RUN/sonic-ring/configdb-before.tsv" ] && st_refuse "$ST_RUN/sonic-ring exists: the ring is up, or an 'up' did not finish ('down' first)"
  [ -e "$ST_RUN/vrf-pairs/configdb-before.tsv" ] && st_refuse "$ST_RUN/vrf-pairs exists: a traffic test runs or was interrupted (macsec-traffic-test.sh --cleanup)"
  [ -x "$SWSSCONFIG" ] || st_refuse "swssconfig not found ($SWSSCONFIG): is sonic-orchagent installed?"
  for p in $P4; do
    v=$(st_hget "INTERFACE|$p" vrf_name); [ -n "$v" ] && st_refuse "CONFIG_DB INTERFACE|$p has vrf_name=$v (a traffic test's leftover: macsec-traffic-test.sh --cleanup)"
    for k in $(st_r "$CDB" --scan --pattern "INTERFACE|$p|*"); do st_refuse "CONFIG_DB $k exists: $p has an address (macsec-traffic-test.sh --cleanup)"; done
    for k in $(st_r "$CDB" --scan --pattern "VLAN_MEMBER|*|$p"); do st_refuse "CONFIG_DB $k exists"; done
  done
  for k in 'VLAN|Vlan100' 'VLAN|Vlan200' 'VRF|Vrf-a' 'VRF|Vrf-b'; do st_has "$CDB" "$k" && st_refuse "CONFIG_DB $k exists"; done
  for k in $(st_r "$CDB" --scan --pattern 'FDB|*' | head -5); do st_refuse "CONFIG_DB static FDB entry $k exists"; done
  for k in $(appl_fdb | head -5); do st_refuse "APPL_DB FDB entry $k exists"; done
  bridge fdb show 2> /dev/null | grep -q -E '02:00:00:00:00:(d1|d2|a1|a2)' && st_refuse "kernel bridge FDB has a ring address (bridge fdb show)"
  while IFS= read -r k; do st_refuse "kernel: $k"; done < <(st_kernel_state | grep -E '^vlan .*Vlan(100|200)|^port .* master=[^ ]|^port .* inet=[^ ]')
  # oper state: the four DAC ports up, nothing else (a port to anything but the loop cables is the house network)
  for k in $(st_r "$APPL" --scan --pattern 'PORT_TABLE:Ethernet*' | LC_ALL=C sort -V); do
    p=${k#PORT_TABLE:} v=$(st_r "$APPL" hget "$k" oper_status)
    case " $P4 " in
      *" $p "*) [ "$v" = up ] || st_refuse "APPL_DB $k oper_status=${v:-none}: a DAC port is not up" ;;
      *) [ "$v" = up ] && st_refuse "APPL_DB $k oper_status=up: only the four DAC ports may be up (house network?)" ;;
    esac
  done
  for p in $P4; do st_has "$APPL" "PORT_TABLE:$p" || st_refuse "APPL_DB PORT_TABLE:$p missing (stack not up?)"; done
  # MKA up wherever a MACsec profile is set (the ring runs with MACsec as SONiC runs it)
  for p in $P4; do
    [ -n "$(st_hget "PORT|$p" macsec)" ] || continue
    for e in EGRESS INGRESS; do
      [ -n "$(st_r "$STATE" --scan --pattern "MACSEC_${e}_SA_TABLE|$p|*" | head -1)" ] || st_refuse "STATE_DB no MACSEC_${e}_SA_TABLE|$p|*: MKA is not up on $p (macsec-up-check.sh)"
    done
  done
  # LLDP: every neighbour is this switch, and each DAC port sees its loop peer
  host=$(st_hget 'DEVICE_METADATA|localhost' hostname); host=${host:-$(hostname)}
  for k in $(st_r "$APPL" --scan --pattern 'LLDP_ENTRY_TABLE:*' | LC_ALL=C sort -V); do
    v=$(st_r "$APPL" hget "$k" lldp_rem_sys_name)
    [ "$v" = "$host" ] || st_refuse "APPL_DB $k: LLDP neighbour '$v' (chassis $(st_r "$APPL" hget "$k" lldp_rem_chassis_id), port $(st_r "$APPL" hget "$k" lldp_rem_port_id)) is not this switch ($host): house-network guard"
  done
  for e in $PEERS; do
    p=${e%%:*} peer=${e#*:} k="LLDP_ENTRY_TABLE:${e%%:*}"
    st_has "$APPL" "$k" || { st_refuse "APPL_DB $k missing: no LLDP neighbour seen on $p yet (LLDP sends every 30 s; or the cable)"; continue; }
    a=$(st_r "$APPL" hget "$k" lldp_rem_port_id) v=$(st_r "$APPL" hget "$k" lldp_rem_port_desc) want=$(st_hget "PORT|$peer" alias)
    [ "$v" = "$peer" ] || { [ -n "$want" ] && [ "$a" = "$want" ]; } || st_refuse "APPL_DB $k: $p sees port '$a' / '$v', expected its loop peer $peer ($want)"
  done
  return 0
}

case "${ST_ARGS[0]:-}" in
check)
  ring_check
  if [ "$ST_REFUSED" = 0 ]; then echo "CHECK OK: the four DAC ports up and looped to themselves (LLDP), nothing else up, no leftovers"
  else echo "CHECK FAIL: $ST_REFUSED finding(s)"; exit 1; fi
  ;;
up)
  ring_check; st_refusals || exit 1
  st_begin sonic-ring arm   # until the end of 'up', a failure restores at once
  for i in $SYSCTL_IFS; do st_x sysctl -q -w "net.ipv6.conf.$i.disable_ipv6=1"; done
  # a port with a router interface cannot be a bridge port: the far ends' INTERFACE entries (mac_addr only) go
  for p in Ethernet4 Ethernet124; do st_w "$CDB" del "INTERFACE|$p"; done
  for p in Ethernet4 Ethernet124; do st_wait_applied "INTERFACE|$p" absent || exit 1; done
  for v in Vlan100 Vlan200; do st_w "$CDB" hset "VLAN|$v" vlanid "${v#Vlan}"; done
  for v in Vlan100 Vlan200; do st_wait_applied "VLAN|$v" present || exit 1; st_x sysctl -q -w "net.ipv6.conf.$v.disable_ipv6=1"; done
  members hset
  for m in $M4; do st_wait_applied "VLAN_MEMBER|${m%%:*}|${m#*:}" present || exit 1; done
  for p in $P4; do st_wait "$p in the kernel bridge" st_kernel_master "$p" Bridge || exit 1; done
  # the kernel has no bridge netfilter: no learning and no flooding on the kernel bridge ports instead, so a CPU
  # copy cannot be bridged back out (vlanmgrd adds no kernel FDB entry for the ring addresses)
  for p in $P4; do st_x bridge link set dev "$p" learning off flood off mcast_flood off bcast_flood off; done
  bridge -d link show | grep -E 'Ethernet(0|4|120|124) |flood|learning' | head -20
  fdb_json SET > "$ST_DIR/fdb.json"; st_x "$SWSSCONFIG" "$ST_DIR/fdb.json"
  # shellcheck disable=SC2016  # eval'd by st_wait
  st_wait "4 static FDB entries in APPL_DB" eval '[ "$(appl_fdb | grep -c -E "^FDB_TABLE:Vlan(100|200):02-00-00-00-00-d[12]$")" = 4 ]' || exit 1
  show vlan brief; D "vlan show"; D "l2 show"
  for p in $P4; do echo "$p $(cat "/sys/class/net/$p/address") ipv6-off=$(sysctl -n "net.ipv6.conf.$p.disable_ipv6")"; done
  ip -br link show type vlan; ip -6 -br addr show | grep -E 'Ethernet|Vlan|Bridge'
  [ "$ST_DRY" = 1 ] && exit 0
  st_disarm; echo "== ring configured; 'down' puts everything back (snapshot $ST_DIR)"
  ;;
inject)  # 740 frames per ring (ring.sh's mix); sizes are frame lengths incl. FCS
  [ -e "$ST_RUN/sonic-ring/configdb-before.tsv" ] || { echo "REFUSED: the ring is not up (no snapshot of 'up' in $ST_RUN/sonic-ring)"; exit 1; }
  python3 - <<'EOF'
import socket
mix = [(64,300),(128,100),(256,100),(512,60),(1024,60),(1518,100),(9000,20)]
for dev, da, sa in (('Ethernet0', 'd1', 'a1'), ('Ethernet4', 'd2', 'a2')):
    s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW); s.bind((dev, 0))
    hdr = bytes.fromhex('0200000000' + da + '0200000000' + sa + '88b5'); n = 0
    for ln, cnt in mix:
        f = hdr + bytes(ln - 4 - len(hdr))
        for i in range(cnt): s.send(f); n += 1
    print(dev, 'sent', n)
EOF
  ;;
sample)  # one timestamped snapshot
  echo "== sample $(date -u +%s) $(date -u +%FT%TZ)"
  for i in $P4; do oid=$(sonic-db-cli COUNTERS_DB hget COUNTERS_PORT_NAME_MAP "$i")
    echo "SAI $i $(sonic-db-cli COUNTERS_DB hmget "COUNTERS:$oid" SAI_PORT_STAT_IF_IN_UCAST_PKTS SAI_PORT_STAT_IF_OUT_UCAST_PKTS SAI_PORT_STAT_IF_IN_OCTETS SAI_PORT_STAT_IF_OUT_OCTETS SAI_PORT_STAT_IF_IN_NON_UCAST_PKTS SAI_PORT_STAT_IF_OUT_NON_UCAST_PKTS SAI_PORT_STAT_IF_IN_DISCARDS SAI_PORT_STAT_IF_IN_ERRORS SAI_PORT_STAT_IF_OUT_DISCARDS SAI_PORT_STAT_IF_OUT_ERRORS | tr '\n' ' ')"; done
  for p in ce0 ce1 ce30 ce31; do echo "ASIC $p $(D "show c Changed Same NonZero $p" | grep -E '^(RPKT|TPKT|RBYT|TBYT|RFCS|RERPKT|RJBR|RFRG|RUND|TERR|RDBGC[0-9]|TDBGC[0-9]|RDISC|UC_PERQ_PKT|MC_PERQ_PKT|DROP)' | awk '{print $1"="$3}' | tr '\n' ' ')"; done
  echo "CPU $(D "show c Changed Same NonZero cpu0" | grep -E '^(TPKT|RPKT|PERQ_PKT)' | awk '{print $1"="$3}' | tr '\n' ' ')"
  for i in $P4; do echo "NETDEV $i rx/tx/rxdrop/txdrop $(cat "/sys/class/net/$i/statistics/"{rx_packets,tx_packets,rx_dropped,tx_dropped} | tr '\n' ' ')"; done
  echo "TEMP asic $(cat /run/asic/temperature) $(python3 -c "
import json; s=json.load(open('/run/fand/state.json'))
print('failsafe=%s' % s['failsafe'], ' '.join('%s:%.1fC/%.0f%%' % (k, z['temperature'], z['duty']) for k, z in s['zones'].items()))")"
  echo "HWMON $(cat /sys/class/hwmon/hwmon*/temp*_input 2>/dev/null | tr '\n' ' ') FANS $(cat /sys/class/hwmon/hwmon*/fan*_input 2>/dev/null | tr '\n' ' ')"
  echo "PROC $(ps -o comm=,pid=,pcpu=,rss=,cputimes= -C syncd,orchagent 2>/dev/null | awk '{printf "%s(%s) cpu%%=%s rss=%sk cpus=%s; ", $1,$2,$3,$4,$5}') load=$(cut -d' ' -f1-3 /proc/loadavg)"
  echo "MKA open-SAs=$(D extmacsec | grep -c ' open ') failed-units=$(systemctl --failed --no-legend | wc -l) oper-up=$(show interfaces status | awk 'NR>2 && $8=="up"' | wc -l)"
  echo "KBRIDGE fwd: $(cat /sys/class/net/Bridge/statistics/tx_packets 2>/dev/null) bridge-tx; fdb entries for d1/d2: $(bridge fdb show 2>/dev/null | grep -c -E '02:00:00:00:00:d[12]')"
  ;;
macsec)  # SONiC MACsec SA counters (keys filtered)
  echo "== macsec $(date -u +%FT%TZ)"
  show macsec 2>&1 | grep -v -i -E 'sak|auth_key|salt' | grep -E '^MACsec port|Egress S|Ingress S|ENCRYPTED  |PKTS_OK|NOT_VALID|INVALID|LATE|UNCHECKED|NOT_USING|UNUSED|CURRENT_XPN|encoding_an'
  ;;
stop)  # ports leave the VLANs: every circulating frame ends at its next ingress (dropped or copied to the CPU once)
  members del
  [ "$ST_DRY" = 1 ] || sleep 8
  show vlan brief
  ;;
down)  # FDB entries first (members before entries would leave orchagent removeBridgePort warnings), then the snapshot
  if appl_fdb | grep -q -E '^FDB_TABLE:Vlan(100|200):02-00-00-00-00-d[12]$'; then
    f=$(mktemp); fdb_json DEL > "$f"; st_x "$SWSSCONFIG" "$f"; rm -f "$f"
    # shellcheck disable=SC2016
    st_wait "static FDB entries gone from APPL_DB" eval '[ -z "$(appl_fdb | grep -E "^FDB_TABLE:Vlan(100|200):02-00-00-00-00-d[12]$")" ]'
  fi
  if [ ! -e "$ST_RUN/sonic-ring/configdb-before.tsv" ] && [ -n "$(st_hget 'INTERFACE|Ethernet4' vrf_name)$(st_hget 'INTERFACE|Ethernet124' vrf_name)" ]; then
    echo "REFUSED: no snapshot, and the far ends carry a traffic test's vrf_name: macsec-traffic-test.sh --cleanup first"; exit 1
  fi
  st_cleanup sonic-ring   # restores, compares, exits
  ;;
*) sed -n '2,25p' "$0"; exit 2;;
esac
