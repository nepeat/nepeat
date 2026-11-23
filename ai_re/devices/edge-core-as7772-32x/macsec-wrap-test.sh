#!/usr/bin/env bash
# Run ON the AS7772 (root, login shell), SONiC up. Does the PHY lose a frame when the low 32 bits of an XPN packet
# number wrap? Static GCM-AES-XPN-256 SAs on cages 1 <-> 2 (the board table through "openbcm-diag extmacsec run", as
# macsec-static-test.sh), installed with a chosen first packet number; pings across it; every SecY counter.
# SONiC's own MACsec is taken off the two ports for the test (CONFIG_DB PORT.macsec) and put back. macsec-loss.md.
#   bash -l macsec-wrap-test.sh DIR SET...      DIR/SET/{a2b,b2a}.par  (pn = first packet number, 8 bytes low first)
# Needs the addresses of macsec-traffic-test.sh. No key is printed.
set -u
dir=${1:?parameter directory}; shift
c=gcm-aes-xpn-256; n=${N:-600}
D() { openbcm-diag "$@"; }
X() { D "extmacsec run $*" | grep -E 'FAILED|rror' ; }
nz() { grep -E '^ *[a-z_0-9]+ +[0-9]+' | awk '$2 != 0 {printf "%s=%s ", $1, $2}'; }
stats() { for s in "ce0 stats-sa.egress.0" "ce1 stats-flow.ingress" "ce1 stats-sa.ingress.0" "ce0 stats-flow.egress" "ce1 stats-sa.egress.0" "ce0 stats-flow.ingress" "ce0 stats-sa.ingress.0" "ce1 stats-flow.egress"; do
            printf '%-26s ' "$s"; D "extmacsec run $s" | nz; echo; done; }
pn() { for s in "ce0 pn.egress.0" "ce1 pn.ingress.0" "ce1 pn.egress.0" "ce0 pn.ingress.0"; do printf '%-18s ' "$s"; D "extmacsec run $s" | tr '\n' ' '; echo; done; }
asic() { for p in ce0 ce1; do D "show c $p" | grep -E '^(RPKT|TPKT|RFCS|RERPKT|TERR|RDISC|RDBGC[0-9]|TDBGC[0-9]|RIPD4|DROP_PKT[A-Z_]*)\.' | awk '{printf "%s=%s ", $1, $3}'; echo; done; }
engines() { echo "SONiC MACsec engines on ce0 / ce1: $(D extmacsec | grep -c -E '^ce[01] ') (0 = both ports left to this test)"; }
sectag() { echo "table $(grep -o 'SAI_OPENBCM_EXT_MACSEC_FILE=.*' "$(systemctl cat sonic-syncd | grep -o '/nix/store/[^ ]*sai.profile' | head -1)" | cut -d= -f2): $(grep -c 'W p 1 9005 5000' "$(grep -o 'SAI_OPENBCM_EXT_MACSEC_FILE=.*' "$(systemctl cat sonic-syncd | grep -o '/nix/store/[^ ]*sai.profile' | head -1)" | cut -d= -f2)") writes of the ingress SecTAG-check register 0x5000 in it (0 = the table before the fix)"; }
echo "== $(date -Is) $(readlink /run/current-system)"
sectag
echo "== 0. SONiC MACsec off on cages 1, 2 (CONFIG_DB PORT.macsec removed)"
prof=$(sonic-db-cli CONFIG_DB hget 'PORT|Ethernet0' macsec)
sonic-db-cli CONFIG_DB hdel 'PORT|Ethernet0' macsec >/dev/null; sonic-db-cli CONFIG_DB hdel 'PORT|Ethernet4' macsec >/dev/null
for i in $(seq 40); do [ "$(D extmacsec | grep -E '^ce[01] ' | grep -c ' on ')" = 0 ] && break; sleep 1; done
engines
ping -q -c 20 -i 0.01 -W 1 10.0.1.1 | tail -2 | head -1
echo "== 1. engines on, gap 48 bytes"
for p in ce0 ce1; do for d in egress ingress; do X $p reset.$d; X $p enable.$d; done; D "m CLMAC_TX_CTRL.$p AVERAGE_IPG=48" >/dev/null; done
first=1
for set in "$@"; do
  echo "== set $set: first packet number (8 bytes, low first) a2b $(grep '^pn ' $dir/$set/a2b.par | cut -d' ' -f2), b2a $(grep '^pn ' $dir/$set/b2a.par | cut -d' ' -f2)"
  if [ $first = 0 ]; then
    X ce0 gate-close.egress; X ce1 gate-close.egress; X ce0 gate-close.ingress; X ce1 gate-close.ingress
    for p in ce0 ce1; do for d in egress ingress; do X $p sa-del-last.$d.0; done; done
  fi
  first=0
  X ce1 sa-add-first.ingress.0.$c $dir/$set/a2b.par; X ce1 gate-open.ingress
  X ce0 sa-add-first.ingress.0.$c $dir/$set/b2a.par; X ce0 gate-open.ingress
  stats >/dev/null; D "clear c" >/dev/null
  X ce0 sa-add-first.egress.0.$c $dir/$set/a2b.par; X ce0 gate-open.egress
  X ce1 sa-add-first.egress.0.$c $dir/$set/b2a.par; X ce1 gate-open.egress
  echo "-- packet numbers before"; pn
  echo "+ ping -c $n -i 0.01 10.0.1.1 (cage 1 -> 2; replies come back 2 -> 1)"
  ping -O -c $n -i 0.01 -W 1 10.0.1.1 | grep -E 'no answer|transmitted' | tr '\n' ';'; echo
  sleep 1
  echo "-- packet numbers after"; pn
  echo "-- SecY counters, non-zero (cleared before the egress SAs were made)"; stats
  echo "-- ASIC"; asic
done
echo "== back to bypass, SONiC's MACsec back on the two ports"
for p in ce0 ce1; do for d in egress ingress; do X $p gate-close.$d; X $p disable.$d; done; D "m CLMAC_TX_CTRL.$p AVERAGE_IPG=16" >/dev/null; done
sonic-db-cli CONFIG_DB hset 'PORT|Ethernet0' macsec "$prof" >/dev/null; sonic-db-cli CONFIG_DB hset 'PORT|Ethernet4' macsec "$prof" >/dev/null
for i in $(seq 60); do [ "$(D extmacsec | grep -c ' open ')" = 8 ] && break; sleep 1; done
D extmacsec | sed -n '1,10p'
ping -q -c 100 -i 0.01 -W 1 10.0.1.1 | tail -2 | head -1
echo "== failed units: $(systemctl --failed --no-legend | wc -l)"
