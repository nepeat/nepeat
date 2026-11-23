#!/usr/bin/env bash
# shellcheck disable=SC2016  # the check conditions are eval'd strings on purpose
# Offline test of the CONFIG_DB snapshot / restore / refusals of macsec-traffic-test.sh, sonic-traffic-test.sh and
# loadtest/sonic-ring.sh (all through sonic-test-lib.sh), against fake.py: redis, a model of intfmgrd / vrfmgrd /
# vlanmgrd and the kernel state they make; fixture.json = a clean boot of the AS7772 (4 DAC ports, MACsec up).
# Proves the scripts' logic only, not SONiC's behaviour on the switch. Workstation, from anywhere:
#   bash devices/edge-core-as7772-32x/loadtest/test/run.sh       exit 0 = every case passed
set -u
T=$(cd "$(dirname "$0")" && pwd); DEV=$(cd "$T/../.." && pwd)
W=$(mktemp -d); trap 'rm -rf "$W"' EXIT
mkdir -p "$W/bin"
for t in redis-cli ip sysctl swssconfig ping bridge; do ln -s "$T/fake.py" "$W/bin/$t"; done
stub() { printf '#!/usr/bin/env bash\n%s\n' "$2" > "$W/bin/$1"; chmod +x "$W/bin/$1"; }
stub sleep ':'
stub openbcm-diag 'case "$1" in extmacsec) for i in 1 2 3 4 5 6 7 8; do echo "ce$i 0 egress on open sa"; done;; esac'
stub show ':'
stub sonic-clear ':'
stub journalctl ':'
stub systemctl 'case "$1" in is-active) for u in "${@:2}"; do echo active; done;; show) echo 0;; esac'
export PATH="$W/bin:$PATH" FAKE_STATE="$W/state.json" ST_RUN="$W/run" ST_REDIS=redis-cli ST_WAIT=1 ST_SETTLE=0 SWSSCONFIG="$W/bin/swssconfig"
nf=0 np=0
rsleep() { python3 -c "import time; time.sleep($1)"; }
reset() { cp "$T/fixture.json" "$FAKE_STATE"; rm -f "$FAKE_STATE".*; rm -rf "$ST_RUN"; }
norm() { python3 -c 'import json, sys; print(json.dumps(json.load(open(sys.argv[1])), sort_keys=True))' "$1"; }
clean() { [ "$(norm "$FAKE_STATE")" = "$(norm "$T/fixture.json")" ]; }   # every database, sysctls, kernel leftovers
r() { redis-cli -n 4 "$@" > /dev/null; }
check() {   # name, condition...
  local name=$1; shift
  if "$@"; then np=$((np + 1)); echo "PASS $name"
  else nf=$((nf + 1)); echo "FAIL $name"; echo "---- output:"; sed 's/^/    /' "$W/out" | tail -40; fi
}
has() { grep -q -F -- "$1" "$W/out"; }
hasnt() { ! grep -q -F -- "$1" "$W/out"; }
run() { "$@" > "$W/out" 2>&1; rc=$?; }
legacy_traffic() {   # what the older traffic tests left: config vrf add / vrf bind / ip add, then mac_addr set again
  r hset 'VRF|Vrf-a' NULL NULL; r hset 'VRF|Vrf-b' NULL NULL
  r del 'INTERFACE|Ethernet4'; r hset 'INTERFACE|Ethernet4' vrf_name Vrf-a
  r del 'INTERFACE|Ethernet124'; r hset 'INTERFACE|Ethernet124' vrf_name Vrf-b
  r hset 'INTERFACE|Ethernet0' NULL NULL; r hset 'INTERFACE|Ethernet120' NULL NULL
  r hset 'INTERFACE|Ethernet0|10.0.1.0/31' NULL NULL; r hset 'INTERFACE|Ethernet4|10.0.1.1/31' NULL NULL
  r hset 'INTERFACE|Ethernet120|10.0.31.0/31' NULL NULL; r hset 'INTERFACE|Ethernet124|10.0.31.1/31' NULL NULL
  r hset 'INTERFACE|Ethernet4' mac_addr 1c:ea:0b:38:2b:d6; r hset 'INTERFACE|Ethernet124' mac_addr 1c:ea:0b:38:2b:d7
}
no_refused_change() { ! grep -q 'can not change' "$FAKE_STATE.managers.log" 2> /dev/null; }

for t in macsec-traffic-test.sh sonic-traffic-test.sh; do
  reset; run bash "$DEV/$t" 10
  check "$t: normal run exits 0" [ "$rc" = 0 ]
  check "$t: restored and verified" has '== RESTORED and verified'
  check "$t: every database / sysctl / kernel state as at boot" clean
  check "$t: no in-place VRF change was attempted" no_refused_change
  check "$t: the far ends were in their VRFs during the test" grep -q 'hset INTERFACE|Ethernet4 vrf_name Vrf-a mac_addr 1c:ea:0b:38:2b:d6' "$FAKE_STATE.writes.log"
done

reset; run bash "$DEV/macsec-traffic-test.sh" --dry-run 10
check "dry-run: exit 0, prints the setup writes" eval '[ "$rc" = 0 ] && has "redis-cli -n 4 hset VRF\|Vrf-a NULL NULL" && has "redis-cli -n 4 hset INTERFACE\|Ethernet0\|10.0.1.0/31 NULL NULL"'
check "dry-run: no write made, no snapshot left" eval '[ ! -e "$FAKE_STATE.writes.log" ] && clean && [ -z "$(ls -A "$ST_RUN" 2> /dev/null)" ]'

set -m
for sig in INT TERM HUP; do
  reset; FAKE_HANG_MARKER=$W/hang FAKE_HANG_SECONDS=5 bash "$DEV/macsec-traffic-test.sh" 10 > "$W/out" 2>&1 & pid=$!
  for _ in $(seq 100); do [ -e "$W/hang" ] && break; rsleep 0.1; done; rm -f "$W/hang"
  kill -"$sig" -- -"$pid"; wait "$pid"; rc=$?
  check "SIG$sig during the pings: restored and verified, non-zero exit ($rc)" eval '[ "$rc" != 0 ] && has "== RESTORED and verified" && clean'
done
reset; FAKE_HANG_MARKER=$W/hang FAKE_HANG_SECONDS=30 bash "$DEV/macsec-traffic-test.sh" 10 > "$W/out" 2>&1 & pid=$!
for _ in $(seq 100); do [ -e "$W/hang" ] && break; rsleep 0.1; done; rm -f "$W/hang"
kill -KILL -- -"$pid"; wait "$pid" 2> /dev/null
set +m
check "SIGKILL: config left, snapshot left" eval '! clean && [ -s "$ST_RUN/vrf-pairs/configdb-before.tsv" ]'
run bash "$DEV/macsec-traffic-test.sh" 10
check "after SIGKILL: the next run refuses, naming the snapshot and the keys" eval '[ "$rc" = 1 ] && has "REFUSED: $ST_RUN/vrf-pairs exists" && has "REFUSED: CONFIG_DB INTERFACE|Ethernet4 has vrf_name=Vrf-a" && has "nothing was changed"'
run bash "$DEV/sonic-traffic-test.sh" --cleanup --dry-run
check "cleanup --dry-run: prints the restore, changes nothing, keeps the snapshot" eval '[ "$rc" = 0 ] && has "redis-cli -n 4 del VRF\|Vrf-a" && ! clean && [ -s "$ST_RUN/vrf-pairs/configdb-before.tsv" ]'
run bash "$DEV/sonic-traffic-test.sh" --cleanup
check "cleanup from the snapshot: restored and verified, snapshot gone" eval '[ "$rc" = 0 ] && has "restoring the snapshot of an interrupted run" && has "== RESTORED and verified" && clean && [ ! -e "$ST_RUN/vrf-pairs" ]'
run bash "$DEV/macsec-traffic-test.sh" --cleanup
check "cleanup again (nothing to do): exit 0, still clean" eval '[ "$rc" = 0 ] && clean'

reset; legacy_traffic
run bash "$DEV/loadtest/sonic-ring.sh" check
check "ring check on old traffic-test leftovers: refuses, names the keys" eval '[ "$rc" = 1 ] && has "CONFIG_DB INTERFACE|Ethernet4 has vrf_name=Vrf-a" && has "CONFIG_DB INTERFACE|Ethernet124|10.0.31.1/31 exists" && has "CONFIG_DB VRF|Vrf-a exists" && has "kernel: port Ethernet4 master=Vrf-a"'
run bash "$DEV/loadtest/sonic-ring.sh" up
check "ring up on those leftovers: refuses, writes nothing" eval '[ "$rc" = 1 ] && has "nothing was changed" && [ ! -e "$ST_RUN/sonic-ring" ]'
run bash "$DEV/macsec-traffic-test.sh" 10
check "traffic test on old leftovers: refuses" eval '[ "$rc" = 1 ] && has "REFUSED: CONFIG_DB VRF|Vrf-a exists"'
run bash "$DEV/macsec-traffic-test.sh" --cleanup
check "cleanup of old leftovers (no snapshot): clean state restored and verified" eval '[ "$rc" = 0 ] && has "no snapshot" && has "== RESTORED and verified" && clean && no_refused_change'

reset; run bash "$DEV/loadtest/sonic-ring.sh" check
check "ring check on a clean boot: OK" eval '[ "$rc" = 0 ] && has "CHECK OK"'
run bash "$DEV/loadtest/sonic-ring.sh" up
check "ring up: configured, snapshot kept for down" eval '[ "$rc" = 0 ] && has "ring configured" && [ -s "$ST_RUN/sonic-ring/configdb-before.tsv" ] && [ "$(redis-cli -n 0 --scan --pattern "FDB_TABLE:*" | wc -l)" = 4 ]'
run bash "$DEV/loadtest/sonic-ring.sh" check
check "ring check while up: refuses" eval '[ "$rc" = 1 ] && has "the ring is up" && has "CONFIG_DB VLAN|Vlan100 exists"'
run bash "$DEV/macsec-traffic-test.sh" 10
check "traffic test while the ring is up: refuses" eval '[ "$rc" = 1 ] && has "the load ring is up"'
run bash "$DEV/loadtest/sonic-ring.sh" stop
run bash "$DEV/loadtest/sonic-ring.sh" down
check "ring stop + down: restored and verified, all databases / sysctls as at boot" eval '[ "$rc" = 0 ] && has "== RESTORED and verified" && has "APPL_DB FDB keys as in the snapshot" && clean'
run bash "$DEV/loadtest/sonic-ring.sh" down
check "ring down again: nothing to do, still clean" eval '[ "$rc" = 0 ] && clean'
reset; run bash "$DEV/loadtest/sonic-ring.sh" up --dry-run
check "ring up --dry-run: prints, changes nothing" eval '[ "$rc" = 0 ] && has "redis-cli -n 4 hset VLAN\|Vlan100 vlanid 100" && clean && [ -z "$(ls -A "$ST_RUN" 2> /dev/null)" ]'

reset   # an 'up' of the older sonic-ring.sh (no snapshot), then down
r del 'INTERFACE|Ethernet4'; r del 'INTERFACE|Ethernet124'; r hset 'VLAN|Vlan100' vlanid 100; r hset 'VLAN|Vlan200' vlanid 200
for m in Vlan100\|Ethernet0 Vlan100\|Ethernet120 Vlan200\|Ethernet4 Vlan200\|Ethernet124; do r hset "VLAN_MEMBER|$m" tagging_mode untagged; done
for i in default Bridge Ethernet0 Ethernet4 Ethernet120 Ethernet124; do sysctl -w "net.ipv6.conf.$i.disable_ipv6=1"; done
fdb=$(mktemp); bash -c ". '$DEV/loadtest/sonic-ring.sh' > /dev/null 2>&1; fdb_json SET" > "$fdb" 2> /dev/null || true
[ -s "$fdb" ] && swssconfig "$fdb"; rm -f "$fdb"
run bash "$DEV/loadtest/sonic-ring.sh" down
check "ring down after an older 'up' (no snapshot): clean state restored and verified" eval '[ "$rc" = 0 ] && has "no snapshot" && has "== RESTORED and verified" && clean'

reset; redis-cli -n 0 hset 'LLDP_ENTRY_TABLE:Ethernet0' lldp_rem_sys_name udm-pro > /dev/null; redis-cli -n 0 hset 'PORT_TABLE:Ethernet8' oper_status up > /dev/null
redis-cli -n 6 del 'MACSEC_INGRESS_SA_TABLE|Ethernet120|1cea0b382bd70001|0' > /dev/null; redis-cli -n 0 del 'LLDP_ENTRY_TABLE:Ethernet124' > /dev/null
rm -f "$FAKE_STATE.writes.log"
run bash "$DEV/loadtest/sonic-ring.sh" up
check "ring guards: foreign LLDP neighbour, another port up, MKA down, LLDP missing: each named, nothing written" eval '[ "$rc" = 1 ] && has "LLDP neighbour '"'"'udm-pro'"'"'" && has "PORT_TABLE:Ethernet8 oper_status=up" && has "MKA is not up on Ethernet120" && has "LLDP_ENTRY_TABLE:Ethernet124 missing" && [ ! -e "$FAKE_STATE.writes.log" ]'

reset; FAKE_PING_DOES='stuck addr Ethernet4 10.0.1.1/31' run bash "$DEV/macsec-traffic-test.sh" 10
check "a kernel address the manager left: FORCED, named, exit 5, state clean" eval '[ "$rc" = 5 ] && has "FORCED: Ethernet4 still had 10.0.1.1/31" && clean'
reset; FAKE_PING_DOES='hset LOGGER|intfmgrd LOGLEVEL DEBUG' run bash "$DEV/macsec-traffic-test.sh" 10
check "a foreign CONFIG_DB change during the test: FAIL, the field named, exit 4, snapshot kept" eval '[ "$rc" = 4 ] && has "CONFIG_DB DIFFERS" && has "LOGGER|intfmgrd" && [ -s "$ST_RUN/vrf-pairs/configdb-before.tsv" ]'
cp "$T/fixture.json" /dev/null
python3 - "$FAKE_STATE" << 'EOF'
import json, sys
s = json.load(open(sys.argv[1])); s['db']['4']['LOGGER|intfmgrd']['v']['LOGLEVEL'] = 'NOTICE'; json.dump(s, open(sys.argv[1], 'w'))
EOF
run bash "$DEV/macsec-traffic-test.sh" --cleanup
check "after the foreign change is undone, --cleanup verifies" eval '[ "$rc" = 0 ] && clean'

echo "== $np passed, $nf failed"
[ "$nf" = 0 ]
