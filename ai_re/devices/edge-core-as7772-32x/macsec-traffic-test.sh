#!/usr/bin/env bash
# Run ON the AS7772 (root, login shell) with SONiC up and MACsec configured (services.sonic.macsec): the traffic test
# of sonic-traffic-test.sh over the two DAC loops with MKA-negotiated MACsec, and SONiC's MACsec counters.
#   bash -l macsec-traffic-test.sh [count]
# Both ends of each link are this switch: the far ends (cages 2, 32) sit in VRFs and get their own MAC address
# (the KaY cannot elect a key server between two ends with one MAC: "duplicate MAC detected"). Runtime config only.
# "show macsec" prints SAKs: everything shown here is filtered (no sak / auth_key / salt lines).
set -u
n=${1:-1000}
export SONIC_CLI_IFACE_MODE=alias
D() { openbcm-diag "$@"; }
macsec() { show macsec "$@" 2>&1 | grep -v -i -E 'sak|auth_key|salt'; }
echo "== $(date -Is) $(readlink /run/current-system)"
echo "== uptime: $(uptime)"
systemctl is-active openbcm-mdio-script sonic-syncd sonic-orchagent sonic-macsecmgrd fand | tr '\n' ' '; echo
echo "== failed units: $(systemctl --failed --no-legend | wc -l)"
ip vrf show | grep -q Vrf-a || config vrf add Vrf-a
ip vrf show | grep -q Vrf-b || config vrf add Vrf-b
sleep 2
ip link show Ethernet4 | grep -q 'master Vrf-a' || config interface vrf bind Ethernet2/1 Vrf-a
ip link show Ethernet124 | grep -q 'master Vrf-b' || config interface vrf bind Ethernet32/1 Vrf-b
sleep 2
ip -4 addr show Ethernet0 | grep -q 10.0.1.0 || config interface ip add Ethernet1/1 10.0.1.0/31
ip -4 addr show Ethernet4 | grep -q 10.0.1.1 || config interface ip add Ethernet2/1 10.0.1.1/31
ip -4 addr show Ethernet120 | grep -q 10.0.31.0 || config interface ip add Ethernet31/1 10.0.31.0/31
ip -4 addr show Ethernet124 | grep -q 10.0.31.1 || config interface ip add Ethernet32/1 10.0.31.1/31
# far ends: the board's 2nd and 3rd MAC (base + 1, + 2)
restart=0
for pm in Ethernet4=1c:ea:0b:38:2b:d6 Ethernet124=1c:ea:0b:38:2b:d7; do
  p=${pm%%=*}; m=${pm#*=}
  [ "$(sonic-db-cli CONFIG_DB hget "INTERFACE|$p" mac_addr)" = "$m" ] || { sonic-db-cli CONFIG_DB hset "INTERFACE|$p" mac_addr "$m" >/dev/null; restart=1; }
done
if [ $restart = 1 ]; then sleep 3; echo "far-end MAC addresses set: restarting macsecmgrd (wpa_supplicant reads the MAC when it starts)"; systemctl restart sonic-macsecmgrd; fi
# wait for MKA: every port has an egress SA installed. After a stack start macsecmgrd can be ahead of intfmgrd
# setting the far ends' MAC (this loop-back topology only): its wpa_supplicants then run with one MAC at both
# ends and never elect a key server; restart it once.
mka() { for i in $(seq $1); do [ "$(D extmacsec | grep -c ' open ')" = 8 ] && return 0; sleep 1; done; return 1; }
t_mka=$(date +%s)
if ! mka 25; then
  journalctl -u sonic-macsecmgrd --since -30s --no-pager | grep -q 'duplicate MAC' && echo "MKA not up after 25 s: 'duplicate MAC detected' (wpa_supplicant started before the far ends had their MAC): restarting macsecmgrd"
  systemctl restart sonic-macsecmgrd; mka 60
fi
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
echo "== MACsec counters (show macsec, since sonic-clear macsec; flex counters poll every 10 s)"
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
