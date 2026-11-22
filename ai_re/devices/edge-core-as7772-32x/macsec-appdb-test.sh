#!/usr/bin/env bash
# Run ON the AS7772 (root, login shell), SONiC up, no MACsec configured in SONiC: static SAs on cages 1 <-> 2
# through SONiC's own path (APPL_DB -> orchagent MACsecOrch -> SAI MACsec objects -> the PHYs), without MKA.
# Stage S2 of macsec.md "Under SONiC".   bash -l macsec-appdb-test.sh DIR   (DIR: appdb-set.json, appdb-del.json
# from evora-rehost/macsec-appdb.py). No key is printed.
set -u
dir=${1:?directory}
D() { openbcm-diag "$@"; }
swssconfig=$(dirname "$(readlink -f "$(systemctl show -p ExecStart --value sonic-orchagent | grep -o '/nix/store/[^ ;]*/bin/orchagent' | head -1)")")/swssconfig
both() { echo "+ ping cage 1 -> 2, 2 -> 1 (200 each)"; ping -q -c 200 -i 0.005 -W 1 10.0.1.1 | tail -2; ip vrf exec Vrf-a ping -q -c 200 -i 0.005 -W 1 10.0.1.0 | tail -2; }
ipg() { for p in ce0 ce1 ce30; do printf '%s %s  ' $p "$(D "g CLMAC_TX_CTRL.$p" | grep -o 'AVERAGE_IPG=[0-9a-fx]*')"; done; echo; }
counters() {   # COUNTERS_DB as syncd's flex counters fill it; names from COUNTERS_MACSEC_NAME_MAP (egress: GB map absent)
  for map in COUNTERS_MACSEC_NAME_MAP; do
    redis-cli -n 2 hgetall $map | paste - - | sort | while read name oid; do
      printf '%s: ' "$name"; redis-cli -n 2 hgetall "COUNTERS:$oid" | paste - - | grep -E 'ENCRYPTED|IN_PKTS_OK|NOT_VALID|INVALID|CURRENT_XPN' | sed 's/SAI_MACSEC_SA_//' | tr '\t\n' '= '; echo
    done
  done
}
echo "== $(date -Is) $(readlink /run/current-system)"
echo "== 0. baseline (bypass)"; both; ipg
echo "== 1. swssconfig appdb-set.json (ports not enabled, SCs, SAs, then ports enabled)"
$swssconfig $dir/appdb-set.json; sleep 4
journalctl -u sonic-syncd -u sonic-orchagent --since -8s --no-pager | grep -i -E 'macsec|SA AN|secure channel|ERR' | cut -d' ' -f3,5- | cut -c1-200
D extmacsec
ipg
ping -c 3 -W 1 10.0.1.1 >/dev/null
both
ping -q -c 200 -i 0.005 -s 1400 10.0.1.1 | tail -2; ping -q -c 100 -i 0.01 -s 8000 10.0.1.1 | tail -2
sleep 3
echo "-- APPL_STATE / STATE_DB"; redis-cli -n 6 keys 'MACSEC*' | sort
echo "-- COUNTERS_DB"; counters
sleep 2; counters | head -2
echo "-- SAI objects in ASIC_DB"; redis-cli -n 1 keys 'ASIC_STATE:SAI_OBJECT_TYPE_MACSEC*' | sed 's/:oid.*//' | sort | uniq -c
echo "   keys in ASIC_DB (SAK attribute present, as SONiC records it): $(redis-cli -n 1 --scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_MACSEC_SA:*' | head -1 | xargs -r redis-cli -n 1 hkeys | grep -c SAK)"
echo "== 2. swssconfig appdb-del.json"
$swssconfig $dir/appdb-del.json; sleep 4
journalctl -u sonic-syncd -u sonic-orchagent --since -8s --no-pager | grep -i -E 'macsec|SA AN|ERR' | cut -d' ' -f3,5- | cut -c1-200
D extmacsec
ipg
sleep 1; both
echo "== failed units: $(systemctl --failed --no-legend | wc -l); orchagent $(systemctl is-active sonic-orchagent), since $(systemctl show -p ActiveEnterTimestamp --value sonic-orchagent)"
