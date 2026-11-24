#!/usr/bin/env bash
# Run ON the AS7772 (root, login shell), SONiC up with MACsec (services.sonic.macsec): SAK changes under traffic.
#   bash -l macsec-rekey-test.sh [--dry-run] [period_s] [seconds]   the test; --dry-run prints the writes, makes none
#   bash -l macsec-rekey-test.sh --cleanup [--dry-run]              undo an interrupted run
# Builds the VRF-pair topology of macsec-traffic-test.sh itself (loadtest/sonic-test-lib.sh beside it or in
# ./loadtest/: CONFIG_DB snapshot, restore on every exit, the whole CONFIG_DB compared with it, refuses over an
# earlier run's leftovers). Sets MACSEC_PROFILE rekey_period at run time (macsecmgrd restarted to take it), floods
# pings both ways over cage 1 <-> 2 (1 ms apart) and 31 -> 32 (10 ms) for the given time, lists every SA change with
# its time. The restore puts the profile back and restarts macsecmgrd again: macsec-up-check.sh then fails on its
# "started once" criterion until the next stack start (by design; -s sonic-macsecmgrd checks from that restart).
# No key is printed.
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
period=${ST_ARGS[0]:-20} secs=${ST_ARGS[1]:-100}
ST_PHASES=('^MACSEC_PROFILE[|]' "${ST_PAIRS_PHASES[@]}")
export SONIC_CLI_IFACE_MODE=alias
D() { openbcm-diag "$@"; }
st_verify_extra() { st_mka_verify && echo "== failed units: $(systemctl --failed --no-legend | wc -l)"; }
st_restore_extra() {   # macsecmgrd reads a profile only when it loads it
  [ -e "$ST_DIR/macsecmgrd-restarted" ] || return 0
  st_x systemctl restart sonic-macsecmgrd
}
[ "$ST_MODE" = cleanup ] && st_cleanup macsec-rekey-test
prof=$(st_hget 'PORT|Ethernet0' macsec)
echo "== $(date -Is) $(readlink /run/current-system)"
st_pairs_macsec_check
[ -n "$prof" ] || st_refuse "CONFIG_DB PORT|Ethernet0 has no MACsec profile"
st_refusals || exit 1
st_pairs_session macsec-rekey-test
echo "== profile $prof: rekey_period $period s, $secs s of pings (1 ms apart, both directions, cage 1 <-> 2)"
: > "$ST_DIR/macsecmgrd-restarted"   # from here on the restore restarts it
st_w "$CDB" hset "MACSEC_PROFILE|$prof" rekey_period "$period"
st_x systemctl restart sonic-macsecmgrd
[ "$ST_DRY" = 1 ] && exit 0
since=$(date '+%Y-%m-%d %H:%M:%S')
st_mka 60 || { echo "FAIL: MKA not up 60 s after the macsecmgrd restart"; D extmacsec; exit 1; }
echo "MKA up after $ST_MKA_S s"; sleep 2
ping -c 3 -W 1 10.0.1.1 >/dev/null
o=$ST_DIR/ping
ping -q -c $((secs * 1000)) -i 0.001 -W 1 10.0.1.1 > "$o-a.txt" 2>&1 & ST_BG+=("$!")
ip vrf exec Vrf-a ping -q -c $((secs * 1000)) -i 0.001 -W 1 10.0.1.0 > "$o-b.txt" 2>&1 & ST_BG+=("$!")
ping -q -c $((secs * 100)) -i 0.01 -W 1 10.0.31.1 > "$o-c.txt" 2>&1 & ST_BG+=("$!")
wait "${ST_BG[@]}"; ST_BG=()
echo "-- cage 1 -> 2"; tail -2 "$o-a.txt"
echo "-- cage 2 -> 1"; tail -2 "$o-b.txt"
echo "-- cage 31 -> 32 (10 ms apart)"; tail -2 "$o-c.txt"
echo "-- SA changes in the PHYs (syncd)"
journalctl -u sonic-syncd --since "$since" --no-pager -o short-precise | grep -E 'SA AN [0-9] (installed|removed)' | sed -E 's/^[A-Za-z]+ [0-9]+ ([0-9:.]+) .*api=43: /\1 /'
echo "-- rekeys per port: $(journalctl -u sonic-syncd --since "$since" --no-pager | grep -c 'egress SA AN [0-9] installed') egress SA installs on 4 ports"
D extmacsec
show macsec Ethernet1/1 2>&1 | grep -E 'encoding_an|SA \(|PKTS_OK|NOT_VALID|INVALID |LATE|UNUSED|NOT_USING|OUT_PKTS_ENCRYPTED|CURRENT_XPN'
journalctl -u sonic-macsecmgrd -u sonic-orchagent -u sonic-syncd --since "$since" --no-pager | grep -i -E ' ERR|WARN|fail' | grep -v -i 'watchdog' | cut -c1-200 | tail -8
echo "== the restore follows: the profile of the snapshot, macsecmgrd restarted, the topology removed"
exit 0
