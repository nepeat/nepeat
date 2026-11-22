#!/usr/bin/env bash
# Runs ON the AS7772 (root). Load test of the two loopback DACs (cage 1 <-> 2, 31 <-> 32) with the ASIC as its
# own traffic generator: two 2-port VLANs and four static L2 entries make two counter-rotating rings
#   d1: ce0 -> cable -> ce1 =switch=> ce31 -> cable -> ce30 =switch=> ce0 ...
#   d2: ce1 -> cable -> ce0 =switch=> ce30 -> cable -> ce31 =switch=> ce1 ...
# Known unicast only (nothing floods, no storm control involved). The CPU injects a few hundred frames per ring;
# the wire holds a handful, the rest waits in the egress queues, so all four ports transmit back to back.
# SONiC is stopped for the test: the SDK is a private bcm.user behind bcmd.py (same directory), which also
# publishes the ASIC temperature for fand. See ../load-test.md.
#   ring.sh up | inject [scale] | sample | stop | down
set -u
here=$(cd "$(dirname "$0")" && pwd)
export OPENBCM_DIAG_SOCKET=/run/m-load/diag.sock
P=ce0,ce1,ce30,ce31
D() { openbcm-diag "$@"; }
D1=02:00:00:00:00:d1; D2=02:00:00:00:00:d2
case "${1:-}" in
up)
  systemctl stop sonic.target
  for i in $(seq 30); do systemctl is-active -q sonic-syncd || break; sleep 1; done
  sleep 12; systemctl stop openbcm-asic-temp      # it comes back when syncd stops; the SDK lock allows one process
  cfg=$(systemctl cat openbcm-mdio-script | grep -o '/nix/store/[^ "]*config\.bcm' | head -1)   # the generated SDK config (MDIO clock, hold delay)
  rm -f $OPENBCM_DIAG_SOCKET; systemctl reset-failed m-load-bcmd 2>/dev/null
  systemd-run --unit m-load-bcmd --collect -E PATH="$PATH" -p StandardOutput=append:/run/m-load-bcmd.out -p StandardError=inherit \
    python3 "$here/bcmd.py" --config "$cfg"
  for i in $(seq 120); do [ -S $OPENBCM_DIAG_SOCKET ] && break; sleep 1; done
  [ -S $OPENBCM_DIAG_SOCKET ] || { echo "bcmd did not come up"; tail -5 /run/m-load-bcmd.out; exit 1; }
  D "g raw CMIC_RATE_ADJUST_EXT_MDIO"; D "g raw CMIC_MIIM_CONFIG"
  D "vlan remove 1 pbm=ce,xe"; D "port ce,xe en=0"
  D "vlan create 100 pbm=ce0,ce30 ubm=ce0,ce30"; D "vlan create 200 pbm=ce1,ce31 ubm=ce1,ce31"
  D "pvlan set ce0,ce30 100"; D "pvlan set ce1,ce31 200"
  D "port $P an=off if=CAUI sp=100000 en=1"
  # IPG=48 for PHYs that are doing MACsec (they add 32 B per frame): ../macsec.md "Under SONiC", load
  for p in ce0 ce1 ce30 ce31; do D "m CLMAC_TX_CTRL.$p AVERAGE_IPG=${IPG:-16}"; D "m CLMAC_RX_MAX_SIZE.$p RX_MAX_SIZE=9130"; done
  sleep 6; D "ps $P"; D "clear c"
  ;;
fwd)   # the four ring entries
  D "l2 add mac=$D1 vlan=200 pbm=ce31 static=true"; D "l2 add mac=$D1 vlan=100 pbm=ce0 static=true"
  D "l2 add mac=$D2 vlan=100 pbm=ce30 static=true"; D "l2 add mac=$D2 vlan=200 pbm=ce1 static=true"
  D "l2 show"
  ;;
inject)  # frames of mixed sizes into both rings; scale multiplies the counts
  k=${2:-1}
  for spec in 64:300 128:100 256:100 512:60 1024:60 1518:100 9000:20; do
    len=${spec%:*}; n=$(( ${spec#*:} * k ))
    D "tx $n pbm=ce0 vlan=200 length=$len DestMac=$D1 SourceMac=02:00:00:00:00:a1"
    D "tx $n pbm=ce1 vlan=100 length=$len DestMac=$D2 SourceMac=02:00:00:00:00:a2"
  done
  ;;
sample)  # one timestamped snapshot: all non-zero counters, temperature, fans
  D "counter sync" >/dev/null
  echo "== sample $(date +%s.%N) $(date -Is)"
  D "show c Changed Same NonZero $P"     # ("show c nz" prints nothing: the 2026-11-22 soak samples have no counters)
  echo "asic $(cat /run/asic/temperature) C; $(D 'show temp' | grep -i 'maximum\|average' | tr '\n' ' ')"
  python3 - <<'EOF'
import json
s = json.load(open('/run/fand/state.json'))
print('fand failsafe=%s ' % s['failsafe'] + ' '.join('%s: %s %.1fC duty %.0f%%' % (k, z['state'], z['temperature'], z['duty']) for k, z in s['zones'].items()))
EOF
  grep -h . /sys/class/hwmon/hwmon*/fan*_input 2>/dev/null | tr '\n' ' '; echo
  grep -h . /sys/class/hwmon/hwmon*/temp*_input 2>/dev/null | tr '\n' ' '; echo
  ;;
stop)  # turn the ring entries into discard entries: every circulating frame is dropped at its next ingress
  for e in "$D1 200 ce31" "$D1 100 ce0" "$D2 100 ce30" "$D2 200 ce1"; do set -- $e
    D "l2 del mac=$1 vlan=$2"; D "l2 add mac=$1 vlan=$2 pbm=$3 static=true dd=true"; done
  sleep 3; D "l2 show"; D "ps $P"
  ;;
soak)  # background sampler (every 30 s) with the thermal guard: stops the ring above 95 C or on fand failsafe
  systemd-run --unit m-load-sampler --collect -E PATH="$PATH" bash -c "while :; do bash $here/ring.sh sample >> /run/m-load/samples.txt 2>&1; t=\$(cut -d. -f1 /run/asic/temperature); if [ \"\$t\" -gt 95 ] || grep -q '\"failsafe\": true' /run/fand/state.json; then echo \"== GUARD: asic \$t C or fand failsafe: stopping the ring\" >> /run/m-load/samples.txt; bash $here/ring.sh stop >> /run/m-load/samples.txt 2>&1; exit 1; fi; sleep 27; done"
  ;;
down)  # leave the chip to SONiC again
  D "port $P en=0" ; D m-quit
  for i in $(seq 30); do systemctl is-active -q m-load-bcmd || break; sleep 1; done
  systemctl start sonic.target
  ;;
*) sed -n '2,12p' "$0"; exit 2;;
esac
