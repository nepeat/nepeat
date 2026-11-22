#!/usr/bin/env bash
# Run ON the AS7772 (root, login shell) with SONiC up: pings across both loopback DACs
# (cage 1 <-> 2 and 31 <-> 32) through the KNET netdevs, each far end in its own VRF,
# with SONiC and ASIC counters. The VRF/IP config is runtime only (no "config save").
#   bash -l sonic-traffic-test.sh [count]
set -u
n=${1:-1000}
export SONIC_CLI_IFACE_MODE=alias
D() { openbcm-diag "$@"; }
echo "== $(date -Is) $(readlink /run/current-system)"
echo "== uptime: $(uptime)"
systemctl is-active openbcm-mdio-script sonic-syncd sonic-orchagent fand | tr '\n' ' '; echo
journalctl -b -u openbcm-mdio-script --no-pager | grep -E 'rate register|done in|FAILED' | cut -d' ' -f6-
echo "== failed units: $(systemctl --failed --no-legend | wc -l)"
show interfaces status 2>&1 | grep -E 'Interface|---| up '
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
sleep 5
ping -c 3 -W 2 10.0.1.1 >/dev/null; ping -c 3 -W 2 10.0.31.1 >/dev/null     # ARP
D clear c >/dev/null
sonic-clear counters >/dev/null 2>&1
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
echo "== SONiC counters"
show interfaces counters -i Ethernet1/1,Ethernet2/1,Ethernet31/1,Ethernet32/1 2>&1
echo "== ASIC counters (frames, bytes, errors)"
for p in ce0 ce1 ce30 ce31; do D show c $p | grep -E '^(RPKT|TPKT|RBYT|TBYT|RFCS|RERPKT|RJBR|ROVR|RFRG|RUND|TERR|RIPD4|RDISC)\.'; done
echo "== ASIC port state"
D ps ce0,ce1,ce30,ce31 | tail -4
for p in ce0 ce30; do D "g CLMAC_TX_CTRL.$p" | grep -o 'AVERAGE_IPG=[0-9a-fx]*'; done
D phy control ce0 | grep -E 'CL74|CL91|Interface'
echo "== temperature / fans"
echo "asic $(cat /run/asic/temperature) C; fand failsafe: $(grep -o '"failsafe": [a-z]*' /run/fand/state.json)"
echo "== lldp"
show lldp table 2>&1 | head -12
