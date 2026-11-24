#!/usr/bin/env bash
# Run ON the AS7772 (root, login shell), SONiC up with MACsec. Does the PHY lose a frame when the low 32 bits of an
# XPN packet number wrap? Static GCM-AES-XPN-256 SAs on cages 1 <-> 2 (the board table through "openbcm-diag
# extmacsec run", as macsec-static-test.sh), installed with a chosen first packet number; pings across it; every
# SecY counter. macsec-loss.md.
#   bash -l macsec-wrap-test.sh [--dry-run] DIR SET...   DIR/SET/{a2b,b2a}.par (pn = first packet number, 8 bytes
#                                                         low first); --dry-run prints the CONFIG_DB writes, makes none
#   bash -l macsec-wrap-test.sh --cleanup [--dry-run]    undo an interrupted run
# Builds the VRF-pair topology of macsec-traffic-test.sh itself (loadtest/sonic-test-lib.sh beside it or in
# ./loadtest/: CONFIG_DB snapshot, restore on every exit, the whole CONFIG_DB compared with it, refuses over an
# earlier run's leftovers). SONiC's own MACsec is taken off the two ports for the test (CONFIG_DB PORT.macsec); on
# every exit the two PHYs go back to bypass (gates closed, engines off, AVERAGE_IPG 16) before PORT.macsec comes
# back from the snapshot, and the check after the restore wants MKA up and AVERAGE_IPG as before. No key is printed.
set -u
here=$(cd "$(dirname "$0")" && pwd)
lib=
for l in "${SONIC_TEST_LIB:-}" "$here/loadtest/sonic-test-lib.sh" "$here/sonic-test-lib.sh"; do
  if [ -n "$l" ] && [ -r "$l" ]; then lib=$l; break; fi
done
# shellcheck disable=SC2218  # the builtin exit here: the library defines exit()
[ -n "$lib" ] || { echo "sonic-test-lib.sh not found: copy loadtest/sonic-test-lib.sh next to this script"; exit 2; }
# shellcheck source=loadtest/sonic-test-lib.sh
. "$lib"
st_args "$@"
OWN=('^PORT[|]Ethernet(0|4)$')
ST_PHASES=("${OWN[@]}" "${ST_PAIRS_PHASES[@]}")
c=gcm-aes-xpn-256 n=${N:-600}
D() { openbcm-diag "$@"; }
X() { D "extmacsec run $*" | grep -E 'FAILED|rror' ; }
nz() { grep -E '^ *[a-z_0-9]+ +[0-9]+' | awk '$2 != 0 {printf "%s=%s ", $1, $2}'; }
stats() { for s in "ce0 stats-sa.egress.0" "ce1 stats-flow.ingress" "ce1 stats-sa.ingress.0" "ce0 stats-flow.egress" "ce1 stats-sa.egress.0" "ce0 stats-flow.ingress" "ce0 stats-sa.ingress.0" "ce1 stats-flow.egress"; do
            printf '%-26s ' "$s"; D "extmacsec run $s" | nz; echo; done; }
pn() { for s in "ce0 pn.egress.0" "ce1 pn.ingress.0" "ce1 pn.egress.0" "ce0 pn.ingress.0"; do printf '%-18s ' "$s"; D "extmacsec run $s" | tr '\n' ' '; echo; done; }
asic() { for p in ce0 ce1; do D "show c $p" | grep -E '^(RPKT|TPKT|RFCS|RERPKT|TERR|RDISC|RDBGC[0-9]|TDBGC[0-9]|RIPD4|DROP_PKT[A-Z_]*)\.' | awk '{printf "%s=%s ", $1, $3}'; echo; done; }
engines() { echo "SONiC MACsec engines on ce0 / ce1: $(D extmacsec | grep -c -E '^ce[01] ') (0 = both ports left to this test)"; }
sectag() { local t; t=$(grep -o 'SAI_OPENBCM_EXT_MACSEC_FILE=.*' "$(systemctl cat sonic-syncd | grep -o '/nix/store/[^ ]*sai.profile' | head -1)" | cut -d= -f2)
           echo "table $t: $(grep -c 'W p 1 9005 5000' "$t") writes of the ingress SecTAG-check register 0x5000 in it (0 = the table before the fix)"; }
ipg() { local p; for p in ce0 ce1; do echo "$p $(D "g CLMAC_TX_CTRL.$p" | grep -o 'AVERAGE_IPG=[0-9a-fx]*')"; done; }
st_snapshot_extra() { ipg > "$ST_DIR/ipg-before.txt"; }
bypass() {   # the two PHYs back to bypass and the MAC gap of bypass, if this test took the engines
  local p d
  [ -e "$ST_DIR/engines-taken" ] || return 0
  echo "== ce0 / ce1 back to bypass (gates closed, engines off), AVERAGE_IPG 16"
  for p in ce0 ce1; do
    for d in egress ingress; do st_x X "$p" "gate-close.$d"; st_x X "$p" "disable.$d"; done
    st_x D "m CLMAC_TX_CTRL.$p AVERAGE_IPG=16" > /dev/null
  done
  rm -f "$ST_DIR/engines-taken"
}
st_restore_pre_extra() { bypass; }
st_verify_extra() {
  st_mka_verify || return 1
  [ -s "$ST_DIR/ipg-before.txt" ] || { echo "== AVERAGE_IPG not compared (no snapshot of it): $(ipg | tr '\n' ' ')"; return 0; }
  if ipg | diff "$ST_DIR/ipg-before.txt" - > /dev/null; then echo "== AVERAGE_IPG of ce0 / ce1 as in the snapshot: $(ipg | tr '\n' ' ')"
  else echo "== FAIL: AVERAGE_IPG differs: before $(tr '\n' ' ' < "$ST_DIR/ipg-before.txt"), now $(ipg | tr '\n' ' ')"; return 1; fi
}
# --cleanup without a snapshot (an older version interrupted): PORT.macsec of cages 1, 2 = cage 31's profile
st_synth_configdb() {
  st_pairs_synth_configdb | awk -F'\t' -v p="$(st_hget 'PORT|Ethernet120' macsec)" '
    $1 ~ /^PORT[|]Ethernet[04]$/ && $2 == "macsec" { seen[$1] = 1 }
    { print }
    END { if (p != "") for (k in want) if (!(k in seen)) print k "\tmacsec\t" p }
    BEGIN { want["PORT|Ethernet0"]; want["PORT|Ethernet4"] }' | LC_ALL=C sort
}
[ "$ST_MODE" = cleanup ] && st_cleanup macsec-wrap-test
[ "${#ST_ARGS[@]}" -ge 2 ] || { sed -n '6,8p' "$0"; exit 2; }
dir=${ST_ARGS[0]}
prof=$(st_hget 'PORT|Ethernet120' macsec)
echo "== $(date -Is) $(readlink /run/current-system)"
st_pairs_macsec_check
[ -n "$prof" ] || st_refuse "CONFIG_DB PORT|Ethernet120 has no MACsec profile"
for p in Ethernet0 Ethernet4; do
  [ "$(st_hget "PORT|$p" macsec)" = "$prof" ] || st_refuse "CONFIG_DB PORT|$p macsec is '$(st_hget "PORT|$p" macsec)', not '$prof' (an older run's leftover? --cleanup)"
done
for set in "${ST_ARGS[@]:1}"; do
  for f in a2b b2a; do grep -q '^pn ' "$dir/$set/$f.par" 2> /dev/null || st_refuse "$dir/$set/$f.par: missing or no 'pn' line"; done
done
st_refusals || exit 1
sectag
st_pairs_session macsec-wrap-test
echo "== 0. SONiC MACsec off on cages 1, 2 (CONFIG_DB PORT.macsec removed)"
st_w "$CDB" hdel 'PORT|Ethernet0' macsec; st_w "$CDB" hdel 'PORT|Ethernet4' macsec
[ "$ST_DRY" = 1 ] && { echo "(dry run: the PHY engine sequence and the pings are not run)"; exit 0; }
for _ in $(seq 40); do [ "$(D extmacsec | grep -E '^ce[01] ' | grep -c ' on ')" = 0 ] && break; sleep 1; done
engines
ping -q -c 20 -i 0.01 -W 1 10.0.1.1 | tail -2 | head -1
echo "== 1. engines on, gap 48 bytes"
: > "$ST_DIR/engines-taken"   # from here on every exit puts the PHYs back to bypass first
for p in ce0 ce1; do for d in egress ingress; do X "$p" "reset.$d"; X "$p" "enable.$d"; done; D "m CLMAC_TX_CTRL.$p AVERAGE_IPG=48" >/dev/null; done
first=1
for set in "${ST_ARGS[@]:1}"; do
  echo "== set $set: first packet number (8 bytes, low first) a2b $(grep '^pn ' "$dir/$set/a2b.par" | cut -d' ' -f2), b2a $(grep '^pn ' "$dir/$set/b2a.par" | cut -d' ' -f2)"
  if [ $first = 0 ]; then
    X ce0 gate-close.egress; X ce1 gate-close.egress; X ce0 gate-close.ingress; X ce1 gate-close.ingress
    for p in ce0 ce1; do for d in egress ingress; do X "$p" "sa-del-last.$d.0"; done; done
  fi
  first=0
  X ce1 "sa-add-first.ingress.0.$c" "$dir/$set/a2b.par"; X ce1 gate-open.ingress
  X ce0 "sa-add-first.ingress.0.$c" "$dir/$set/b2a.par"; X ce0 gate-open.ingress
  stats >/dev/null; D "clear c" >/dev/null
  X ce0 "sa-add-first.egress.0.$c" "$dir/$set/a2b.par"; X ce0 gate-open.egress
  X ce1 "sa-add-first.egress.0.$c" "$dir/$set/b2a.par"; X ce1 gate-open.egress
  echo "-- packet numbers before"; pn
  echo "+ ping -c $n -i 0.01 10.0.1.1 (cage 1 -> 2; replies come back 2 -> 1)"
  ping -O -c "$n" -i 0.01 -W 1 10.0.1.1 | grep -E 'no answer|transmitted' | tr '\n' ';'; echo
  sleep 1
  echo "-- packet numbers after"; pn
  echo "-- SecY counters, non-zero (cleared before the egress SAs were made)"; stats
  echo "-- ASIC"; asic
done
bypass
echo "== SONiC's MACsec back on the two ports (PORT.macsec from the snapshot)"
st_restore_configdb "$ST_DIR/configdb-before.tsv" "${OWN[@]}" || exit 1
rc=0
if st_mka 60; then echo "MKA up after $ST_MKA_S s"; else echo "FAIL: MKA not back 60 s after PORT.macsec"; rc=1; fi
D extmacsec | sed -n '1,10p'
ping -q -c 100 -i 0.01 -W 1 10.0.1.1 | tail -2 | head -1
echo "== failed units: $(systemctl --failed --no-legend | wc -l)"
exit $rc
