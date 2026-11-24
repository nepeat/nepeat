#!/usr/bin/env bash
# shellcheck disable=SC2016  # the check conditions are eval'd strings on purpose
# Offline test of the CONFIG_DB snapshot / restore / refusals of macsec-traffic-test.sh, sonic-traffic-test.sh,
# macsec-{rekey,wrongcak,wrap,clear-window}-test.sh and loadtest/sonic-ring.sh (all through sonic-test-lib.sh), and
# of their exit status under `bash -l` (a logout file that reads an unset variable), against fake.py: redis, a model
# of intfmgrd / vrfmgrd / vlanmgrd and the kernel state they make, the PHYs' MKA / engines / MAC gap,
# sonic-macsec-config; fixture.json = a clean boot of the AS7772 (4 DAC ports, MACsec up).
# Proves the scripts' logic only, not SONiC's behaviour on the switch. Workstation, from anywhere:
#   bash devices/edge-core-as7772-32x/loadtest/test/run.sh       exit 0 = every case passed
set -u
T=$(cd "$(dirname "$0")" && pwd); DEV=$(cd "$T/../.." && pwd)
W=$(mktemp -d)
kill_left() { [ -s "$FAKE_STATE.pids" ] && xargs kill < "$FAKE_STATE.pids" 2> /dev/null; rm -f "$FAKE_STATE.pids"; }   # hung fake pings
trap 'kill_left; rm -rf "$W"' EXIT
mkdir -p "$W/bin" "$W/home" "$W/par/s1"
for t in redis-cli ip sysctl swssconfig ping bridge openbcm-diag sonic-macsec-config.py; do ln -s "$T/fake.py" "$W/bin/$t"; done
stub() { printf '#!/usr/bin/env bash\n%s\n' "$2" > "$W/bin/$1"; chmod +x "$W/bin/$1"; }
stub sleep ':'
stub show ':'
stub sonic-clear ':'
stub journalctl ':'
# sonic-macsec-config's ExecStart (python, script, declaration) points at the fake; restarts are logged
cat > "$W/bin/systemctl" << EOF
#!/usr/bin/env bash
case "\$1" in
  is-active) for u in "\${@:2}"; do echo active; done ;;
  show) if [ "\${3:-}" = ExecStart ]; then echo "{ path=python3 ; argv[]=python3 -I $W/bin/sonic-macsec-config.py $W/decl.json ; ignore_errors=no }"; else echo 0; fi ;;
  restart|start|stop) echo "\$*" >> "$W/systemctl.log" ;;
esac
EOF
chmod +x "$W/bin/systemctl"
# macsec-clear-window-test.sh's macsec-up-check.sh: PASS, or (FAKE_UC_HANG=file) its pid in the file, then blocked on a
# FIFO nobody writes until it is killed
mkfifo "$W/never"
printf '#!/usr/bin/env bash\n[ -n "${FAKE_UC_HANG:-}" ] && { echo $$ > "$FAKE_UC_HANG.new"; mv "$FAKE_UC_HANG.new" "$FAKE_UC_HANG"; read -r _ < "%s"; }\necho "PASS: MKA up (offline stub)"; echo end\n' "$W/never" > "$W/up-check.sh"
# the lab profile of fixture.json (its primary_cak is this key, type 7); wrong-test's key: the test's own random one
cak=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
(umask 077 && echo "$cak" > "$W/lab.cak")
printf '{"profiles": {"lab-test": {"cakFile": "%s", "cakHexDigits": 64, "fields": {"cipher_suite": "GCM-AES-XPN-256", "priority": "0", "send_sci": "true"}}}, "ports": {"Ethernet0": "lab-test", "Ethernet4": "lab-test", "Ethernet120": "lab-test", "Ethernet124": "lab-test"}, "stateFile": "%s"}\n' "$W/lab.cak" "$W/macsec-state.json" > "$W/decl.json"
printf 'pn 0100000000000000\n' > "$W/par/s1/a2b.par"; cp "$W/par/s1/a2b.par" "$W/par/s1/b2a.par"
echo 'echo "logout reads $NOT_SET_IN_ANY_LOGOUT" > /dev/null' > "$W/home/.bash_logout"   # as NixOS /etc/bash_logout does
export PATH="$W/bin:$PATH" FAKE_STATE="$W/state.json" ST_RUN="$W/run" ST_REDIS=redis-cli ST_WAIT=1 ST_SETTLE=0 SWSSCONFIG="$W/bin/swssconfig" MACSEC_UP_CHECK="$W/up-check.sh"
nf=0 np=0
rsleep() { python3 -c "import time; time.sleep($1)"; }
reset() { kill_left; cp "$T/fixture.json" "$FAKE_STATE"; rm -f "$FAKE_STATE".* "$W/systemctl.log" "$W/macsec-state.json" "$W/hang" "$W/uc.pid"; rm -rf "$ST_RUN"; }
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
login() { HOME="$W/home" bash --noprofile -l "$@" > "$W/out" 2>&1; rc=$?; }   # the documented `bash -l script`
pings_left() { local p; [ -s "$FAKE_STATE.pids" ] || return 1; while read -r p; do kill -0 "$p" 2> /dev/null && return 0; done < "$FAKE_STATE.pids"; return 1; }
nwrites() { cat "$FAKE_STATE.writes.log" 2> /dev/null | wc -l; }
no_key() { ! grep -q -i -E "$cak|[0-9a-f]{40}" "$W/out"; }   # neither the lab key nor any long hex string (a key, type 7)
marker() { for _ in $(seq 100); do [ -e "$1" ] && { cat "$1"; rm -f "$1"; return 0; }; rsleep 0.1; done; return 1; }   # polls an observable file; prints it
bg() { "$@" > "$W/out" 2>&1 & pid=$!; }
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

# ---- the MACsec tests that need the VRF-pair topology build their own (sonic-test-lib.sh) ----
# per script: name (its snapshot), arguments, the ping its INT / TERM / KILL cases hang in (FAKE_HANG_MATCH, _NTH)
# shellcheck disable=SC2034  # read through declare -n
rekey=(macsec-rekey-test "20 2" "-i 0.001" 1)
# shellcheck disable=SC2034
wrongcak=(macsec-wrongcak-test "" "-c 100 -i 0.01 -W 1 10.0.1.1" 2)
# shellcheck disable=SC2034
wrap=(macsec-wrap-test "$W/par s1" "-O" 1)
for v in rekey wrongcak wrap; do
  declare -n S=$v; tn=${S[0]} sh=$DEV/${S[0]}.sh; read -r -a args <<< "${S[1]}"   # tn: check() has a local "name"
  reset; run bash "$sh" "${args[@]}"
  check "$tn: normal run exits 0, restored and verified, every state as at boot" eval '[ "$rc" = 0 ] && has "== RESTORED and verified" && has "== MKA: 8 SAs open" && clean && no_refused_change'
  check "$tn: the far ends were in their VRFs during the test" grep -q 'hset INTERFACE|Ethernet4 vrf_name Vrf-a mac_addr 1c:ea:0b:38:2b:d6' "$FAKE_STATE.writes.log"
  check "$tn: no key printed" no_key
  case $v in
    rekey) check "$tn: rekey_period written, macsecmgrd restarted by the test and by the restore" eval 'grep -q "hset MACSEC_PROFILE|lab-test rekey_period 20" "$FAKE_STATE.writes.log" && [ "$(grep -c "restart sonic-macsecmgrd" "$W/systemctl.log")" = 2 ] && has "MKA up after"' ;;
    wrongcak) check "$tn: wrong profile on Ethernet4 by sonic-macsec-config, MKA down on cage 1-2 then back" eval 'grep -q "hset PORT|Ethernet4 macsec wrong-test" "$FAKE_STATE.writes.log" && grep -q "hset MACSEC_PROFILE|wrong-test" "$FAKE_STATE.writes.log" && has "ce0 egress gate closed" && has "MKA up after"' ;;
    wrap) check "$tn: engines taken, back to bypass, IPG and PORT.macsec as before" eval 'grep -q "extmacsec run ce0 enable.egress" "$FAKE_STATE.diag.log" && grep -q "extmacsec run ce1 disable.ingress" "$FAKE_STATE.diag.log" && has "AVERAGE_IPG of ce0 / ce1 as in the snapshot"' ;;
  esac
  reset; run bash "$sh" --dry-run "${args[@]}"
  check "$tn: dry-run: exit 0, prints the setup and its own writes, makes none" eval '[ "$rc" = 0 ] && has "redis-cli -n 4 hset VRF\|Vrf-a NULL NULL" && [ ! -e "$FAKE_STATE.writes.log" ] && clean && [ -z "$(ls -A "$ST_RUN" 2> /dev/null)" ]'
  set -m
  for sig in INT TERM; do
    reset; FAKE_HANG_MARKER=$W/hang FAKE_HANG_MATCH=${S[2]} FAKE_HANG_NTH=${S[3]} FAKE_HANG_SECONDS=20 bg bash "$sh" "${args[@]}"
    marker "$W/hang" > /dev/null
    if [ "$v" = rekey ] && [ "$sig" = TERM ]; then kill -TERM "$pid"; to=script; else kill -"$sig" -- -"$pid"; to=group; fi
    wait "$pid"; rc=$?
    check "$tn: SIG$sig to the $to mid-run: restored and verified, non-zero exit ($rc), no ping left" eval '[ "$rc" != 0 ] && has "== RESTORED and verified" && clean && ! pings_left'
  done
  reset; FAKE_HANG_MARKER=$W/hang FAKE_HANG_MATCH=${S[2]} FAKE_HANG_NTH=${S[3]} FAKE_HANG_SECONDS=20 bg bash "$sh" "${args[@]}"
  marker "$W/hang" > /dev/null; kill -KILL -- -"$pid"; wait "$pid" 2> /dev/null
  set +m
  kill_left
  check "$tn: SIGKILL: config left, snapshot left" eval '! clean && [ -s "$ST_RUN/$tn/configdb-before.tsv" ]'
  run bash "$sh" "${args[@]}"
  check "$tn: after SIGKILL: the next run refuses, naming the snapshot" eval '[ "$rc" = 1 ] && has "REFUSED: $ST_RUN/$tn exists" && has "nothing was changed"'
  run bash "$DEV/macsec-traffic-test.sh" 10
  check "$tn: after SIGKILL: the traffic test refuses too" eval '[ "$rc" = 1 ] && has "REFUSED: $ST_RUN/$tn exists: $tn.sh did not finish"'
  run bash "$DEV/loadtest/sonic-ring.sh" check
  check "$tn: after SIGKILL: the ring check refuses too" eval '[ "$rc" = 1 ] && has "REFUSED: $ST_RUN/$tn exists"'
  # shellcheck disable=SC2034  # read in the eval below
  n0=$(nwrites); run bash "$sh" --cleanup --dry-run
  check "$tn: cleanup --dry-run: prints the restore, changes nothing, keeps the snapshot" eval '[ "$rc" = 0 ] && has "redis-cli -n 4 del VRF\|Vrf-a" && [ "$(nwrites)" = "$n0" ] && ! clean && [ -s "$ST_RUN/$tn/configdb-before.tsv" ]'
  : > "$W/systemctl.before"; [ -e "$W/systemctl.log" ] && cp "$W/systemctl.log" "$W/systemctl.before"
  run bash "$sh" --cleanup
  check "$tn: cleanup from the snapshot: restored and verified, state as at boot, snapshot gone" eval '[ "$rc" = 0 ] && has "restoring the snapshot of an interrupted run" && has "== RESTORED and verified" && clean && [ ! -e "$ST_RUN/$tn" ]'
  case $v in
    rekey) check "$tn: the cleanup restarted macsecmgrd (the snapshot's profile loaded)" eval '[ "$(grep -c "restart sonic-macsecmgrd" "$W/systemctl.log")" = $(($(grep -c "restart sonic-macsecmgrd" "$W/systemctl.before") + 1)) ]' ;;
    wrap) check "$tn: the cleanup put the PHYs back to bypass first" eval 'has "back to bypass" && has "run X ce0 disable.egress"' ;;
  esac
done
reset; r hset 'MACSEC_PROFILE|wrong-test' cipher_suite GCM-AES-XPN-256 primary_cak "07$cak"; r hset 'PORT|Ethernet4' macsec wrong-test
run bash "$DEV/macsec-wrongcak-test.sh"
check "wrongcak: an older run's wrong profile (no snapshot): refused, named" eval '[ "$rc" = 1 ] && has "MACSEC_PROFILE|wrong-test exists" && has "PORT|Ethernet4 macsec is '"'"'wrong-test'"'"'"'
run bash "$DEV/macsec-wrongcak-test.sh" --cleanup
check "wrongcak: --cleanup without a snapshot: wrong profile gone, Ethernet4 on lab-test, no key printed" eval '[ "$rc" = 0 ] && has "no snapshot" && has "== RESTORED and verified" && clean && no_key'
reset; r hdel 'PORT|Ethernet0' macsec; r hdel 'PORT|Ethernet4' macsec
run bash "$DEV/macsec-wrap-test.sh" "$W/par" s1
check "wrap: an older run's PORT.macsec removal (no snapshot): refused, named" eval '[ "$rc" = 1 ] && has "PORT|Ethernet0 macsec is '"''"', not '"'"'lab-test'"'"'"'
run bash "$DEV/macsec-wrap-test.sh" --cleanup
check "wrap: --cleanup without a snapshot: PORT.macsec back, MKA up, state as at boot" eval '[ "$rc" = 0 ] && has "no snapshot" && has "== RESTORED and verified" && clean'
reset; FAKE_PING_DOES='del MACSEC_PROFILE|lab-test' run bash "$DEV/macsec-rekey-test.sh" 20 2
check "rekey: the profile deleted under it: re-created from the snapshot, its key not printed" eval '[ "$rc" = 0 ] && has "primary_cak <not shown>" && no_key && clean'
reset; FAKE_PING_DOES="hset MACSEC_PROFILE|other primary_cak 07${cak}ff" run bash "$DEV/macsec-wrongcak-test.sh"
check "wrongcak: a foreign key change: FAIL, exit 4, the field named, no key printed" eval '[ "$rc" = 4 ] && has "CONFIG_DB DIFFERS" && has "primary_cak	<not shown>" && no_key'

cw=$DEV/macsec-clear-window-test.sh
reset; run bash "$cw" true
check "clear-window: normal run: exit 0 (the check's), report printed, restored and verified, no ping left" eval '[ "$rc" = 0 ] && has "== IN CLEAR" && has "PASS: MKA up" && has "== RESTORED and verified" && clean && ! pings_left'
reset; run bash "$cw" --dry-run true
check "clear-window: dry-run: exit 0, prints the setup, makes no write" eval '[ "$rc" = 0 ] && has "redis-cli -n 4 hset VRF\|Vrf-a NULL NULL" && [ ! -e "$FAKE_STATE.writes.log" ] && clean && [ -z "$(ls -A "$ST_RUN" 2> /dev/null)" ]'
set -m
reset; FAKE_UC_HANG=$W/uc.pid bg bash "$cw" true; marker "$W/uc.pid" > /dev/null; kill -INT -- -"$pid"; wait "$pid"; rc=$?
check "clear-window: SIGINT to the group across the restart: restored and verified, exit $rc, no ping left" eval '[ "$rc" = 130 ] && has "== RESTORED and verified" && clean && ! pings_left'
reset; FAKE_UC_HANG=$W/uc.pid bg bash "$cw" true; uc=$(marker "$W/uc.pid")
kill -TERM "$pid"; kill -TERM "$uc"; wait "$pid"; rc=$?
check "clear-window: SIGTERM to the script (the check ends): the ping and the capture stopped, restored, exit $rc" eval '[ "$rc" = 143 ] && has "== RESTORED and verified" && clean && ! pings_left'
reset; FAKE_UC_HANG=$W/uc.pid bg bash "$cw" true; marker "$W/uc.pid" > /dev/null; kill -KILL -- -"$pid"; wait "$pid" 2> /dev/null
set +m
kill_left
check "clear-window: SIGKILL: snapshot left" eval '! clean && [ -s "$ST_RUN/macsec-clear-window-test/configdb-before.tsv" ]'
run bash "$cw" true
check "clear-window: after SIGKILL: refuses, naming the snapshot" eval '[ "$rc" = 1 ] && has "REFUSED: $ST_RUN/macsec-clear-window-test exists"'
run bash "$cw" --cleanup --dry-run
check "clear-window: cleanup --dry-run: changes nothing" eval '[ "$rc" = 0 ] && ! clean && [ -s "$ST_RUN/macsec-clear-window-test/configdb-before.tsv" ]'
run bash "$cw" --cleanup
check "clear-window: cleanup: restored and verified, state as at boot" eval '[ "$rc" = 0 ] && has "== RESTORED and verified" && clean && [ ! -e "$ST_RUN/macsec-clear-window-test" ]'

# ---- bash -l: the exit status is the script's own (a logout file that reads an unset variable, as on NixOS) ----
printf 'set -u\nexit 0\n' > "$W/bare.sh"; login "$W/bare.sh"
check "login shell: the simulated logout file bites a bare set -u script (exit 1, unbound variable)" eval '[ "$rc" = 1 ] && has "unbound variable"'
reset; login "$DEV/macsec-up-check.sh" -x
check "login shell: macsec-up-check.sh bad option: exit 2" [ "$rc" = 2 ]
reset; login "$DEV/macsec-traffic-test.sh" 10
check "login shell: macsec-traffic-test.sh normal: exit 0" eval '[ "$rc" = 0 ] && has "== RESTORED and verified"'
reset; FAKE_PING_DOES='stuck addr Ethernet4 10.0.1.1/31' login "$DEV/macsec-traffic-test.sh" 10
check "login shell: macsec-traffic-test.sh FORCED: exit 5" [ "$rc" = 5 ]
reset; login "$DEV/macsec-rekey-test.sh" 20 2
check "login shell: macsec-rekey-test.sh normal: exit 0" eval '[ "$rc" = 0 ] && has "== RESTORED and verified"'
for v in rekey wrongcak wrap; do
  declare -n S=$v; read -r -a args <<< "${S[1]}"
  reset; login "$DEV/${S[0]}.sh" --dry-run "${args[@]}"
  check "login shell: ${S[0]}.sh --dry-run: exit 0" [ "$rc" = 0 ]
done
reset; login "$cw" true
check "login shell: macsec-clear-window-test.sh normal: exit 0" eval '[ "$rc" = 0 ] && has "== RESTORED and verified"'
reset; login "$DEV/loadtest/sonic-ring.sh" check
check "login shell: sonic-ring.sh check: exit 0" eval '[ "$rc" = 0 ] && has "CHECK OK"'
reset; login "$DEV/loadtest/sonic-ring.sh" up
check "login shell: sonic-ring.sh up: exit 0" eval '[ "$rc" = 0 ] && has "ring configured"'
run bash "$DEV/loadtest/sonic-ring.sh" down

echo "== $np passed, $nf failed"
[ "$nf" = 0 ]
