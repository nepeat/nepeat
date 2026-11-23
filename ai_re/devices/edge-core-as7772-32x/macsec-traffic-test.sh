#!/usr/bin/env bash
# Run ON the AS7772 (root, login shell) with SONiC up and MACsec configured (services.sonic.macsec): the traffic test
# of sonic-traffic-test.sh over the two DAC loops with MKA-negotiated MACsec, and SONiC's MACsec counters.
#   bash -l macsec-traffic-test.sh [--dry-run] [count]    the test; --dry-run prints the CONFIG_DB writes, makes none
#   bash -l macsec-traffic-test.sh --cleanup [--dry-run]   undo an interrupted run of this or of sonic-traffic-test.sh
# Needs loadtest/sonic-test-lib.sh beside it (or in ./loadtest/). Both ends of each link are this switch: the far ends
# (cages 2, 32) go into VRFs Vrf-a / Vrf-b and keep their own MAC address (the KaY cannot elect a key server between
# two ends with one MAC: "duplicate MAC detected"). Runtime config only, put back exactly: CONFIG_DB snapshot before
# the first write, restore on every exit, then the whole CONFIG_DB and the kernel state compared with the snapshot
# (sonic-test-lib.sh); a run refuses to start over an earlier run's leftovers. Not yet run on the switch in this form.
# MKA has to be up by itself: nothing is restarted here, and the test fails when a port has no SA after 60 s.
# "show macsec" prints SAKs: everything shown here is filtered (no sak / auth_key / salt lines).
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
n=${ST_ARGS[0]:-1000}
ST_PHASES=("${ST_PAIRS_PHASES[@]}")
export SONIC_CLI_IFACE_MODE=alias
D() { openbcm-diag "$@"; }
macsec() { show macsec "$@" 2>&1 | grep -v -i -E 'sak|auth_key|salt'; }
mka() { for _ in $(seq "$1"); do [ "$(D extmacsec | grep -c ' open ')" = 8 ] && return 0; sleep 1; done; return 1; }
st_verify_extra() {   # MKA again on all four ports after the restore (re-creating a far end's entry sets its MAC)
  if mka 60; then echo "== MKA: 8 SAs open after the restore"
  else echo "== FAIL: MKA not up on all four ports 60 s after the restore"; D extmacsec; return 1; fi
}
[ "$ST_MODE" = cleanup ] && st_cleanup vrf-pairs
echo "== $(date -Is) $(readlink /run/current-system)"
echo "== uptime: $(uptime)"
systemctl is-active openbcm-mdio-script sonic-syncd sonic-orchagent sonic-macsecmgrd fand | tr '\n' ' '; echo
echo "== failed units: $(systemctl --failed --no-legend | wc -l)"
st_pairs_check
for p in Ethernet4 Ethernet124; do
  want=FAR_MAC_$p; have=$(st_hget "INTERFACE|$p" mac_addr)
  [ "$have" = "${!want}" ] || st_refuse "CONFIG_DB INTERFACE|$p mac_addr is '$have', not ${!want}: MACsec needs the far end's own address (--cleanup puts it back)"
done
st_refusals || exit 1
st_begin vrf-pairs arm
echo "== setup: VRFs, far ends into them, addresses (CONFIG_DB)"
st_pairs_up || { echo "FAIL: setup did not complete"; exit 1; }
[ "$ST_DRY" = 1 ] && exit 0
# MKA must be up BY ITSELF: this test restarts nothing. It waits 60 s for an egress SA on every port and fails
# without one (macsec-up-check.sh says why and how long the start took).
t_mka=$(date +%s)
if ! mka 60; then
  echo "FAIL: MKA is not up on all four ports after 60 s and this test does not restart anything"
  D extmacsec
  journalctl -t wpa_supplicant --since -90s --no-pager | grep KaY | grep -v -i -E 'sak|key=|cak' | cut -c1-160 | tail -6
  exit 1
fi
starts=$(journalctl -b -o short-monotonic --no-pager -u sonic-macsecmgrd | sed -E 's/^\[ *([0-9.]+)\].*(--- Starting macsecmgrd ---).*/\1 \2/' | awk -v t="$(systemctl show -P InactiveExitTimestampMonotonic sonic-syncd)" '/Starting macsecmgrd/ && $1 >= t / 1e6' | wc -l)
[ "$starts" -le 1 ] || echo "NOTE: macsecmgrd was started $starts times since the stack start: this run does not show that MKA came up by itself"
echo "== MACsec up on all four ports $(( $(date +%s) - t_mka )) s after the check began"; D extmacsec
show interfaces status 2>&1 | grep -E 'Interface|---| up '
macsec --mka 2>&1 | head -40
ip neigh flush dev Ethernet0; ip neigh flush dev Ethernet120
ping -c 3 -W 2 10.0.1.1 >/dev/null; ping -c 3 -W 2 10.0.31.1 >/dev/null     # ARP
sleep 11; sonic-clear macsec >/dev/null 2>&1; D "clear c" >/dev/null; sonic-clear counters >/dev/null 2>&1
echo "== ping cage 1 -> 2 ($n x 64 B, then $n x 1400 B, then 200 x 8000 B)"
ping -q -c "$n" -i 0.002 10.0.1.1 | tail -2
ping -q -c "$n" -i 0.002 -s 1400 10.0.1.1 | tail -2
ping -q -c 200 -i 0.005 -s 8000 10.0.1.1 | tail -2
echo "== ping cage 2 -> 1 (Vrf-a)"
ip vrf exec Vrf-a ping -q -c "$n" -i 0.002 10.0.1.0 | tail -2
echo "== ping cage 31 -> 32"
ping -q -c "$n" -i 0.002 10.0.31.1 | tail -2
ping -q -c "$n" -i 0.002 -s 1400 10.0.31.1 | tail -2
ping -q -c 200 -i 0.005 -s 8000 10.0.31.1 | tail -2
echo "== ping cage 32 -> 31 (Vrf-b)"
ip vrf exec Vrf-b ping -q -c "$n" -i 0.002 10.0.31.0 | tail -2
sleep 12
echo "== MACsec SA counters (show macsec: counted from each SA's install, not reset by sonic-clear macsec; flex counters poll every 10 s)"
macsec | grep -E '^MACsec port|Egress S|Ingress S|encoding_an|ENCRYPTED  |PKTS_OK|NOT_VALID|INVALID|LATE|UNCHECKED|CURRENT_XPN'
echo "== per link: frames encrypted by the sender = frames accepted by the receiver?"
python3 - <<'EOF'
import subprocess, re
out = subprocess.run(['show', 'macsec'], capture_output=True, text=True, env=dict(__import__('os').environ, SONIC_CLI_IFACE_MODE='default')).stdout
port = sc = kind = None
tx, rx, bad = {}, {}, 0
for l in out.splitlines():
    m = re.match(r'MACsec port\((\S+)\)', l)
    if m: port = m.group(1)
    m = re.search(r'MACsec (Egress|Ingress) SC \((\w+)\)', l)
    if m: kind, sc = m.groups()
    m = re.search(r'SAI_MACSEC_SA_STAT_(\w+)\s+(\d+)', l)
    if m and sc:
        name, v = m.group(1), int(m.group(2))
        if kind == 'Egress' and name == 'OUT_PKTS_ENCRYPTED': tx[sc] = tx.get(sc, 0) + v
        if kind == 'Ingress' and name == 'IN_PKTS_OK': rx[sc] = rx.get(sc, 0) + v
        if kind == 'Ingress' and name in ('IN_PKTS_NOT_VALID', 'IN_PKTS_INVALID', 'IN_PKTS_LATE', 'IN_PKTS_UNCHECKED', 'IN_PKTS_NOT_USING_SA', 'IN_PKTS_UNUSED_SA', 'IN_PKTS_DELAYED'): bad += v
for sc in sorted(tx):
    print('SC %s: encrypted %d, accepted by the peer %d: %s' % (sc, tx[sc], rx.get(sc, -1), 'equal' if tx[sc] == rx.get(sc) else 'DIFFERENT (counters are read 1 s apart; EAPOL is not counted)'))
print('ingress error counters, all SAs: %d' % bad)
EOF
echo "== SONiC counters"
show interfaces counters -i Ethernet1/1,Ethernet2/1,Ethernet31/1,Ethernet32/1 2>&1
echo "== ASIC counters (frames, bytes, errors)"
for p in ce0 ce1 ce30 ce31; do D show c $p | grep -E '^(RPKT|TPKT|RBYT|TBYT|RFCS|RERPKT|RJBR|ROVR|RFRG|RUND|TERR|RDISC)\.'; done
echo "== ASIC port state"
D ps ce0,ce1,ce30,ce31 | tail -4
for p in ce0 ce1 ce30 ce31 ce2; do printf '%s %s  ' $p "$(D "g CLMAC_TX_CTRL.$p" | grep -o 'AVERAGE_IPG=[0-9a-fx]*')"; done; echo
D extphy | sed -n '1,4p'
echo "== temperature / fans"
echo "asic $(cat /run/asic/temperature) C; fand failsafe: $(grep -o '"failsafe": [a-z]*' /run/fand/state.json)"
echo "== lldp"
show lldp table 2>&1 | head -8
