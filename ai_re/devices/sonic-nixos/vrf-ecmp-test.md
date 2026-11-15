# VRF + ECMP on the AS7760: switch test (design, 2026-11-15)

Lab loop: Ethernet1/1 (Ethernet0) <-> Ethernet32/1 (Ethernet124), 25G. One
cable, both ends on this switch: Ethernet32/1 goes into `Vrf-test`,
Ethernet1/1 stays in the default VRF, a /31 between them. Every frame a
KNET netdev sends leaves its own port without touching the pipeline; the
ASIC only sees it at the other end, so each test below says what the
ingress lookup there must do. Both netdevs carry the switch MAC
(8c:ea:1b:89:c9:3c). No FRR needed (APPL_DB routes via swssconfig).

Needs: a system with NET_VRF (kernel config change: **reboot**),
`sonic-vrfmgrd`, the sai-openbcm with VRF/ECMP, sonic-utilities patch 0004
(alias lists in `show ... -i`). Ethernet1/1 and Ethernet32/1 in no
PortChannel/VLAN, admin up, link up. [l3_inject.py](l3_inject.py) copied to
the switch.

```sh
# 0. baseline
zcat /proc/config.gz | grep -E '^CONFIG_(NET_VRF|IP_MULTIPLE_TABLES|IPV6_MULTIPLE_TABLES|NET_L3_MASTER_DEV)='
systemctl is-active sonic-vrfmgrd; lsmod | grep -w vrf
journalctl -b -u sonic-syncd | grep 'SDK tables'          # ECMP groups x paths, member entries, VRF ids
journalctl -b -u sonic-orchagent | grep -E 'ECMP group'   # "Maximum number of ECMP groups supported is <N>", N > 0
SWSS=$(systemctl show -p ExecStart --value sonic-orchagent | grep -o '/nix/store/[^ ;]*-sonic-swss-[^/ ;]*' | head -1)
A() { redis-cli -s /var/run/redis/redis.sock -n 1 "$@"; }

# 1. VRF
config vrf add Vrf-test
config interface vrf bind Ethernet32/1 Vrf-test
config interface ip add Ethernet1/1 10.0.0.0/31
config interface ip add Ethernet32/1 10.0.0.1/31
ip -d link show Vrf-test | grep 'vrf table'                # table 1001
ip link show Ethernet124 | grep 'master Vrf-test'
A --scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_VIRTUAL_ROUTER:*' | wc -l   # 2
journalctl -b -u sonic-syncd | grep -E 'virtual router: SDK VRF|rif type .* vrf'   # "SDK VRF 1", the Ethernet124 RIF "vrf 1"

# 2. across the VRFs (trap path: IP2ME at the far end, replies ttl=64)
sonic-clear counters
ping -c 100 -i 0.01 10.0.0.1                               # default VRF -> Vrf-test: 0% loss
ip vrf exec Vrf-test ping -c 100 -i 0.01 10.0.0.0          # Vrf-test -> default: 0% loss
sleep 3; show interfaces counters -i Ethernet1/1,Ethernet32/1
#   Ethernet1/1 TX_OK ~200 (+ARP), Ethernet32/1 RX_OK ~200, and the same the other way
# optional, if iperf3 is on the switch (CPU path both ends: Mb/s, not line rate):
ip vrf exec Vrf-test iperf3 -s -B 10.0.0.1 -D; iperf3 -c 10.0.0.1 -B 10.0.0.0 -t 10
```

Why step 2 proves the VRFs in the chip: after the pings each kernel has
learned the other side's address (neighsyncd -> SAI neighbor), so VRF 0 holds
a host entry 10.0.0.1 -> port Ethernet0, while 10.0.0.1/32 is IP2ME only in
VRF 1. An echo request to 10.0.0.1 enters at Ethernet124: looked up in VRF 1
it goes to the CPU (reply). If the RIF's VLAN were not bound to VRF 1, the
VRF 0 host entry would send it back out Ethernet0 (a loop until TTL, ping
fails). The same holds the other way for 10.0.0.0 and the host entries'
VRF. (Connected /31 subnet routes reach the ASIC only through zebra ->
fpmsyncd; without FRR only IP2ME /32s and the learned neighbors are there,
which is all these tests need.)

```sh
# 3. routed in the chip: VRF route through an ECMP group, positive end to end
#    Vrf-test: 192.0.2.0/24 via 10.0.0.0 + 10.0.1.0 (both = Ethernet0, back over the
#    cable); default VRF owns 192.0.2.1 (Loopback0). A raw frame 10.0.0.1 -> 192.0.2.1
#    sent out of Ethernet0 enters at Ethernet124 (VRF 1), is routed by the ASIC out
#    of Ethernet124 (TTL 64 -> 63), enters at Ethernet0 (VRF 0), IP2ME -> CPU.
config interface ip add Ethernet1/1 10.0.1.0/31
config interface ip add Ethernet32/1 10.0.1.1/31
config interface ip add Loopback0 192.0.2.1/32
ip vrf exec Vrf-test ping -c 2 10.0.0.0; ip vrf exec Vrf-test ping -c 2 10.0.1.0   # neighbors in VRF 1
cat > /tmp/r1.json <<'EOF'
[{"ROUTE_TABLE:Vrf-test:192.0.2.0/24": {"nexthop": "10.0.0.0", "ifname": "Ethernet124"}, "OP": "SET"}]
EOF
$SWSS/bin/swssconfig /tmp/r1.json                          # 3a: one next hop first
tcpdump -ni Ethernet0 -c 20 -v 'icmp and host 192.0.2.1' & sleep 1
sonic-clear counters
python3 l3_inject.py --dev Ethernet0 --src 10.0.0.1 --dst 192.0.2.1 --count 20
#   tcpdump: 20 echo requests IN on Ethernet0 with "ttl 63" (+ 20 replies out); none with ttl 64
#   counters: Ethernet1/1 TX ~40 RX ~20, Ethernet32/1 RX ~40 TX ~20
cat > /tmp/r2.json <<'EOF'
[{"ROUTE_TABLE:Vrf-test:192.0.2.0/24": {"nexthop": "10.0.0.0,10.0.1.0", "ifname": "Ethernet124,Ethernet124"}, "OP": "SET"}]
EOF
$SWSS/bin/swssconfig /tmp/r2.json                          # 3b: ECMP
A --scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_NEXT_HOP_GROUP:*' | wc -l          # 1
A --scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER:*' | wc -l   # 2
A --scan --pattern '*ROUTE_ENTRY*192.0.2.0/24*'            # its NEXT_HOP_ID = the group oid:
A hget "$(A --scan --pattern '*ROUTE_ENTRY*192.0.2.0/24*')" SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID
# repeat the tcpdump + injection: same result (ttl 63) through the SDK ECMP group;
# both members egress Ethernet124, so the hash spread is not visible on one cable.
journalctl -b -u sonic-syncd | grep -iE 'ecmp|next-hop group'   # no errors

# 3c: negative control: drop the route, inject again -> tcpdump sees nothing on
# Ethernet0, Ethernet32/1 TX does not grow (VRF 1 has no route: dropped, or the
# Vrf-test kernel answers "net unreachable" if the chip sends misses to the CPU).
cat > /tmp/r3.json <<'EOF'
[{"ROUTE_TABLE:Vrf-test:192.0.2.0/24": {}, "OP": "DEL"}]
EOF
$SWSS/bin/swssconfig /tmp/r3.json

# 4. teardown
config interface ip remove Loopback0 192.0.2.1/32         # (vtysh only if FRR is absent: else
config interface ip remove Ethernet1/1 10.0.1.0/31        #  delete the INTERFACE keys with redis-cli -n 4)
config interface ip remove Ethernet1/1 10.0.0.0/31
config interface vrf unbind Ethernet32/1                   # removes its addresses
config vrf del Vrf-test
A --scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_VIRTUAL_ROUTER:*' | wc -l   # 1
```

Open on hardware: the BCM56930 VRF binding (`l3a_vrf` on the L3
interface of the internal VLAN), ECMP group create/replace with the SDK's
default group size, and the free ECMP member count (SDK software table) are
unproven; step 0's log lines give the chip's real ECMP and VRF limits.

## debug: SDK state while syncd owns the chip (2026-11-15)

First try ([vrf-hw-try1-20261115.txt](vrf-hw-try1-20261115.txt)): every
unicast frame to the router MAC (IPv4, ARP replies) was dropped in the chip,
broadcast ARP and a unicast ARP request got through. Fixed in sai-openbcm
(unverified): IPv4/IPv6 L3 enabled on every front-panel port at SDK init
(PORT_TAB V4L3/V6L3_ENABLE, nothing set them before), the RIF VLAN's L3
enable checked, and the my-station entry also terminates ARP (ARP_RARP, which
the ARP reply trap needs). syncd's start log now says "IPv4/IPv6 L3 on N
ports" and "my-station <sid>: 8c:ea:1b:89:c9:3c (IPv4, IPv6, ARP)".

`openbcm-diag <command>` runs one SDK diag-shell command inside syncd
(`SAI_OPENBCM_DIAG_SOCKET=/run/sai-openbcm/diag.sock`, root only; exit,
init, reset, rcload, `!` and `;` are refused). The output comes back on
stdout; if a command prints nothing, look in `journalctl -u sonic-syncd`.
Ethernet1/1 = xe0 (logical port 50), Ethernet32/1 = xe4 (logical 13) per the
syncd log; `openbcm-diag ps` confirms.

```sh
D=openbcm-diag
SID=$(journalctl -b -u sonic-syncd | grep -o 'my-station [0-9]*' | tail -1 | cut -d' ' -f2)
$D l2 station show id=$SID       # MAC=8c:ea:1b:89:c9:3c, MAC MASK=ff:ff:ff:ff:ff:ff, VLAN MASK=0x0,
                                 # Flags include 0x1c (IPV4|IPV6|ARP_RARP; the old build 0xc)
$D dump chg MY_STATION_TCAM      # one valid entry, IPV4/IPV6/ARP_RARP_TERMINATION_ALLOWED=1
$D dump chg PORT_TAB 50 1        # V4L3_ENABLE=1 V6L3_ENABLE=1 (absent = 0 = the old bug)
$D dump chg PORT_TAB 13 1
$D l3 intf show                  # 2 interfaces, VLAN 4093 (xe0, VRF 0) and 4094 (xe4, VRF 1), MAC 8c:ea:1b:89:c9:3c
$D vlan show 4093                # xe0 untagged + cpu0
$D vlan show 4094                # xe4 untagged + cpu0
$D dump chg VLAN_TAB 4093 2      # VRF_ID (or L3_IIF) for 4093 -> 0, 4094 -> 1; note VLAN_PROFILE_PTR
$D dump chg VLAN_PROFILE_TAB     # that profile: IPV4L3_ENABLE=1 IPV6L3_ENABLE=1
$D dump chg L3_IIF 4093 2        # VRF 0 / VRF 1 if the chip binds the VRF here
$D l3 defip show                 # VRF 0: 10.0.0.0/31, 10.0.0.0/32; VRF 1: 10.0.0.0/31, 10.0.0.1/32 (all to the CPU egress)
$D l3 l3table show               # learned neighbors (10.0.0.1 in VRF 0, 10.0.0.0 in VRF 1) once ARP works
$D l3 egress show                # CPU egress (L2TOCPU, port cpu0), drop egress, one per neighbor
$D l2 show                       # the router MAC learned on xe0/xe4 in 4093/4094 from the looped frames:
                                 # expected, harmless while the my-station entry hits
$D sc ArpRequestToCpu; $D sc ArpReplyToCpu; $D sc L3EgressMode   # all 1
$D sc DosAttackMACSAEqualMACDA   # 1 would drop SA == DA (every looped unicast frame: both ends share the MAC)

# which counter takes the drops
$D clear c
ping -c 20 -i 0.05 10.0.0.1; ip vrf exec Vrf-test ping -c 20 -i 0.05 10.0.0.0
$D show c ch xe0; $D show c ch xe4; $D show c ch cpu0
#   working: RIPC4 (IPv4 routed/terminated) +20 and more on xe0 and xe4, cpu0 transmit +~40,
#   pings 0% loss. Broken: RIPD4 (IPv4 L3 discard), RDBGC*, RPORTD or RDISC grow instead:
#   send the three outputs.
```

Experiment if pings still fail and DosAttackMACSAEqualMACDA reads 1:
`$D sc DosAttackMACSAEqualMACDA=0`, ping again, set it back to 1 (a lab-loop
artefact, not a SAI bug, if that fixes it). `clear c` also zeroes what
syncd's counter polling reads.

