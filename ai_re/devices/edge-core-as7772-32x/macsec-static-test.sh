#!/usr/bin/env bash
# Run ON the AS7772 (root, login shell) with SONiC up and no MACsec configured in SONiC: static MACsec SAs on
# cages 1 <-> 2, installed by hand with the board's external MACsec table through syncd's diag socket
# ("openbcm-diag extmacsec run <port> <sequence> [<parameter file>]"), i.e. the sequences and the executor that
# the SAI MACsec objects use, without SONiC's MACsec stack. Stage S1 of macsec.md "Under SONiC".
#   bash -l macsec-static-test.sh DIR [cipher]     DIR: a2b.par b2a.par wrong.par (evora-rehost/macsec-params.py)
# Needs the addresses of sonic-traffic-test.sh (10.0.1.0/31 on cages 1 and 2, cage 2 in Vrf-a). No key is printed.
set -u
dir=${1:?parameter directory}; c=${2:-gcm-aes-xpn-256}
D() { openbcm-diag "$@"; }
X() { echo "+ extmacsec run $*" | sed 's|/.*/||'; D "extmacsec run $*"; }
P() { echo "+ ping $*"; ping -q "$@" | tail -2; }
both() { P -c 200 -i 0.005 -W 1 10.0.1.1; echo "+ (Vrf-a)"; ip vrf exec Vrf-a ping -q -c 200 -i 0.005 -W 1 10.0.1.0 | tail -2; }
stats() { for s in "ce0 stats-sa.egress.0" "ce1 stats-sa.ingress.0" "ce1 stats-sa.egress.0" "ce0 stats-sa.ingress.0"; do
            echo "+ $s"; D "extmacsec run $s" | grep -E 'FAILED|out_pkts_encrypted|octets_encrypted|in_pkts_ok|in_pkts_not_valid|in_pkts_invalid|in_pkts_late|in_pkts_unchecked' | tr '\n' ' '; echo; done; }
echo "== $(date -Is) $(readlink /run/current-system)"
echo "== 0. baseline (bypass)"; both
D extmacsec | head -3
echo "== 1. engines on, control frames in clear, data dropped (gate closed); ASIC inter-frame gap 48 bytes"
for p in ce0 ce1; do for d in egress ingress; do X $p reset.$d; X $p enable.$d; done; D "m CLMAC_TX_CTRL.$p AVERAGE_IPG=48" >/dev/null; done
both
echo "== 2. SAs: receiver first, then the transmitter; gate open"
X ce1 sa-add-first.ingress.0.$c $dir/a2b.par; X ce1 gate-open.ingress
X ce0 sa-add-first.egress.0.$c $dir/a2b.par; X ce0 gate-open.egress
X ce0 sa-add-first.ingress.0.$c $dir/b2a.par; X ce0 gate-open.ingress
X ce1 sa-add-first.egress.0.$c $dir/b2a.par; X ce1 gate-open.egress
sleep 1; ping -c 3 -W 1 10.0.1.1 >/dev/null; stats >/dev/null   # ARP; empty the counters
D "clear c" >/dev/null
both
P -c 200 -i 0.005 -s 1400 10.0.1.1; P -c 100 -i 0.01 -s 8000 10.0.1.1
stats
for s in "ce0 pn.egress.0" "ce1 pn.ingress.0"; do echo "+ $s"; D "extmacsec run $s" | tr '\n' ' '; echo; done
echo "-- ASIC counters"; for p in ce0 ce1; do D "show c $p" | grep -E '^(RPKT|TPKT|RBYT|TBYT|RFCS|RERPKT|TERR)\.'; done
echo "-- link state (external PHY table)"; D extphy | sed -n '1,4p'
echo "== 3. wrong key on the receiver of cage 1 -> 2"
X ce1 gate-close.ingress; X ce1 sa-del-last.ingress.0; X ce1 sa-add-first.ingress.0.$c $dir/wrong.par; X ce1 gate-open.ingress
stats >/dev/null; D "clear c" >/dev/null
P -c 100 -i 0.005 -W 1 10.0.1.1
stats
D "show c ce1" | grep -E '^(RPKT|RFCS|RERPKT)\.'
echo "-- the right key again"
X ce1 gate-close.ingress; X ce1 sa-del-last.ingress.0; X ce1 sa-add-first.ingress.0.$c $dir/a2b.par; X ce1 gate-open.ingress
sleep 1; both
echo "== 4. link state with MACsec on: far end admin down / up"
export SONIC_CLI_IFACE_MODE=alias
config interface shutdown Ethernet2/1; sleep 3; show interfaces status 2>/dev/null | grep -E 'Ethernet(1|2)/1 '
config interface startup Ethernet2/1; sleep 5; show interfaces status 2>/dev/null | grep -E 'Ethernet(1|2)/1 '
journalctl -u sonic-syncd --since -25s --no-pager | grep -E 'line side|fault' | cut -d' ' -f3,7- | tail -6
sleep 2; both
echo "== 5. back to bypass"
for p in ce0 ce1; do for d in egress ingress; do X $p gate-close.$d; X $p disable.$d; done; D "m CLMAC_TX_CTRL.$p AVERAGE_IPG=16" >/dev/null; done
sleep 1; both
D extphy | sed -n '1,4p'
echo "== failed units: $(systemctl --failed --no-legend | wc -l)"
