# BFD on sonic-nixos (software BFD, 2026-11-15)

Design and CONFIG_DB usage: [module README, "BFD (software)"](../../nix/modules/switch/sonic/README.md#bfd-software).
Why software: the BCM56930's BFD offload runs on an on-chip microcontroller
with closed Broadcom firmware; sai-openbcm implements no SAI BFD. orchagent's
BfdOrch asks the SAI for `SAI_SWITCH_ATTR_SUPPORTED_IPV4/IPV6_BFD_SESSION_OFFLOAD_TYPE`;
sai-openbcm's capability table does not list them (get not implemented), so
BfdOrch logs "BFD offload type not implemented" (NOTICE), never creates a SAI
BFD object and copies APPL_DB BFD_SESSION_TABLE to STATE_DB
BFD_SOFTWARE_SESSION_TABLE instead. Nothing errors; in the VM (vs SAI told the
same) ASIC_DB has no BFD object.

## what is upstream, what is ours

| piece | from |
| --- | --- |
| bfdd | FRR 10.7.1 unpatched (SONiC carries no bfdd patch); `sonic-frr-bfdd`, no config file |
| BGP_NEIGHBOR `bfd`, `bfd_check_ctrl_plane_failure` | YANG upstream; rendered only by frrcfgd upstream, here by bgpcfgd's template (patch 0002) and applied live on change (0003) |
| staticroutebfd, BfdOrch, BfdMgr | upstream; BfdMgr sessions active unless `type=async_passive` (0003; upstream passive), staticroutebfd timers from `services.sonic.bfd.staticRoute` (0003; upstream fixed 50 ms x3) |
| bfdsyncd (bfdd state -> STATE_DB BFD_SESSION_TABLE) | ours: upstream has no reporter for software sessions, so staticroutebfd never saw Up (routes never installed) and `show bfd summary` stayed empty |
| bfdmon | not run (writes DPU_BFD_PROBE_STATE for smart-switch DPUs) |

## VM result (tests/vm-bfd.nix, 96/96 twice, [evidence](sonic-bfd-vm-20261115.txt))

Times are from the fault (test driver clock) to the effect, range over runs.

| what | measured |
| --- | --- |
| BGP over a LACP PortChannel, 100% loss at the peer (LACP stays up) | BGP down 0.66-0.80 s, `lastResetDueTo: BFD down initiated` |
| BGP over a port in Vrf-test, peer link down (switch port stays oper up) | BGP down 0.74-0.90 s |
| same, BFD turned off live (`bfd=false`) | BGP still Established 5 s later (hold time 90 s) |
| static route with BFD (default VRF), peer sends AdminDown | out of APPL_DB ROUTE_TABLE in 0.1-0.3 s; back 0.7-0.9 s after `no shutdown` |
| static route with BFD in Vrf-test, peer link down | out of APPL_DB in 0.89-1.13 s (detection + bfdsyncd poll + staticroutebfd) |
| bfdsyncd killed / bfdd restarted | states and routes come back; orchagent untouched |
| CPU, 4 sessions at 300 ms x3 (x86 VM) | bfdd 0.5 %, bfdsyncd 0.1 % of a core |

Detection is 3 x 300 ms from the last packet received, so 0.6 to 0.9 s after a
fault. Found on the way: orchagent's BfdMonitorOrch parses STATE_DB
BFD_SESSION_TABLE (strict field list), and staticroutebfd needs ~1 s to stop, so
nothing may be ordered after it (else `config reload` raced; vm-frr caught it). Cross-build: [sonic-bfd-cross-20261115.txt](sonic-bfd-cross-20261115.txt) (+1.2 MB).

## switch test (AS7760; run 2026-11-15, [result](sonic-as7760-bfd-20261115.txt))

Lab loop as in [vrf-ecmp-test.md](vrf-ecmp-test.md): Ethernet1/1 (Ethernet0,
default VRF, 10.0.0.0/31) <-> Ethernet32/1 (Ethernet124, Vrf-test, 10.0.0.1/31),
both ends on this switch, so every BFD packet crosses the cable and is trapped
to the CPU at the far port (IP2ME), like BGP was. Deploy the current
`as7760-system` first (no kernel change; BFD is UDP + userspace).

```sh
# 0. units, seed, no offload, firewall
systemctl is-active sonic-frr-bfdd sonic-bfdsyncd sonic-staticroutebfd sonic-bgpcfgd   # 4x active
sonic-db-cli CONFIG_DB hget 'SYSTEM_DEFAULTS|software_bfd' status                     # enabled
journalctl -b -u sonic-bgpcfgd | grep -c 'software_bfd feature is enabled' || systemctl restart sonic-bgpcfgd   # >= 1 (bgpcfgd reads it at start)
journalctl -b -u sonic-orchagent | grep -i 'BFD offload'      # "BFD offload type not implemented" (IPv4 and IPv6)
nft list ruleset | grep 3784                                   # iifname != { "eth0" } udp dport { 3784, 3785, 4784 } accept

# 1. VRF, addresses, iBGP with BFD (re-creates the 2026-11-15 BGP setup)
sonic-db-cli CONFIG_DB hset 'DEVICE_METADATA|localhost' bgp_asn 65100
config loopback add Loopback0; config interface ip add Loopback0 10.255.0.1/32
systemctl restart sonic-frr-config                             # bgp_asn/Loopback0 into bgpd.conf
config vrf add Vrf-test
config interface vrf bind Ethernet32/1 Vrf-test
config interface ip add Ethernet1/1 10.0.0.0/31
config interface ip add Ethernet32/1 10.0.0.1/31
config interface startup Ethernet1/1; config interface startup Ethernet32/1
sonic-db-cli CONFIG_DB hset 'BGP_NEIGHBOR|10.0.0.1' asn 65100 name a local_addr 10.0.0.0 admin_status up keepalive 30 holdtime 90 bfd true
sonic-db-cli CONFIG_DB hset 'BGP_NEIGHBOR|Vrf-test|10.0.0.0' asn 65100 name b local_addr 10.0.0.1 admin_status up keepalive 30 holdtime 90 bfd true
sleep 30
vtysh -c 'show running-config' | grep -E 'router bgp|neighbor 10.0.0.[01] bfd'   # both "neighbor ... bfd" lines
vtysh -c 'show bfd peers brief'      # 2 sessions up: 10.0.0.0->10.0.0.1 (default), 10.0.0.1->10.0.0.0 (Vrf-test)
vtysh -c 'show bfd peers'            # interface Ethernet0 / Ethernet124, 300 ms tx/rx, multiplier 3
show bfd summary                     # 2 sessions, State Up, Type async_active, 300 300 3 false
show ip bgp summary; vtysh -c 'show bgp vrf Vrf-test summary'                    # both Established
```

If the sessions stay Down while BGP is up: `vtysh -c 'show bfd peers counters'`
(input packets rising = bfdd drops them, e.g. TTL != 255 if the ASIC decremented
it on the trap path; `tcpdump -ni Ethernet124 -v udp port 3784` shows the TTL);
no input = trap/CoPP or firewall. Report either; do not change CoPP blindly.

```sh
# 2. BFD detection with the link up: drop BFD into Vrf-test only (BGP's TCP keeps flowing)
T0=$(date '+%F %T.%N'); nft insert rule inet nixos-fw input iifname '{ "Ethernet124", "Vrf-test" }' udp dport 3784 drop
sleep 5
journalctl -o short-precise --since "$T0" -u sonic-frr-bgpd -u sonic-bfdsyncd | grep -E 'ADJCHANGE|bfdsyncd:'; echo "T0 $T0"
#   expected: Vrf-test|Ethernet124|10.0.0.0 Up -> Down (diag control detection time expired) 0.6-0.9 s after T0,
#   default|Ethernet0|10.0.0.1 Up -> Down (neighbor signaled session down) right after,
#   two "%ADJCHANGE: neighbor ... Down BFD down received" within ~1 s of T0
vtysh -c 'show bgp vrf all summary'                  # both not Established
nft -a list chain inet nixos-fw input | grep 'dport 3784 drop'     # note handle N
nft delete rule inet nixos-fw input handle N
sleep 20; show bfd summary; show ip bgp summary      # Up again; Established within ~10-15 s (connect timer 10)

# 3. port shutdown (carrier loss: kernel link down, not BFD detection)
T0=$(date '+%F %T.%N'); config interface shutdown Ethernet32/1
sleep 5; journalctl -o short-precise --since "$T0" -u sonic-frr-bgpd -u sonic-frr-bfdd -u sonic-bfdsyncd -u sonic-portmgrd | grep -E 'ADJCHANGE|bfdsyncd:|Ethernet124'; echo "T0 $T0"
#   expected: both sessions Down and BGP Down within ~1 s (zebra reports the interface down to bfdd/bgpd;
#   linkscan latency adds to it); compare with step 2
config interface startup Ethernet32/1; sleep 20; show bfd summary; show ip bgp summary

# 4. static routes with BFD, VRF-aware; BGP BFD off first (live toggle; one next hop, one mechanism)
sonic-db-cli CONFIG_DB hset 'BGP_NEIGHBOR|10.0.0.1' bfd false
sonic-db-cli CONFIG_DB hset 'BGP_NEIGHBOR|Vrf-test|10.0.0.0' bfd false
sleep 5; vtysh -c 'show running-config' | grep -c 'neighbor 10.0.0.[01] bfd'    # 0; show bfd summary: 0 sessions
sonic-db-cli CONFIG_DB hset 'STATIC_ROUTE|default|198.51.100.0/24' nexthop 10.0.0.1 ifname Ethernet0 bfd true
sonic-db-cli CONFIG_DB hset 'STATIC_ROUTE|Vrf-test|198.51.101.0/24' nexthop 10.0.0.0 ifname Ethernet124 bfd true
sleep 10
show bfd summary                     # 2 sessions Interface "default", Up, local 10.0.0.0 / 10.0.0.1, 300 300 3
redis-cli -s /var/run/redis/redis.sock -n 0 hgetall ROUTE_TABLE:198.51.100.0/24            # nexthop 10.0.0.1 ifname Ethernet0
redis-cli -s /var/run/redis/redis.sock -n 0 hgetall ROUTE_TABLE:Vrf-test:198.51.101.0/24   # nexthop 10.0.0.0 ifname Ethernet124
redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern '*ROUTE_ENTRY*198.51.10*'      # 2 entries
openbcm-diag 'l3 defip show'                                     # both prefixes in the chip
T0=$(date '+%F %T.%N'); nft insert rule inet nixos-fw input iifname '{ "Ethernet124", "Vrf-test" }' udp dport 3784 drop
sleep 3; redis-cli -s /var/run/redis/redis.sock -n 0 exists ROUTE_TABLE:198.51.100.0/24 ROUTE_TABLE:Vrf-test:198.51.101.0/24   # 0
journalctl -o short-precise --since "$T0" -u sonic-bfdsyncd -u sonic-staticroutebfd | grep -E 'bfdsyncd:|delete static route'
nft -a list chain inet nixos-fw input | grep 'dport 3784 drop'; nft delete rule inet nixos-fw input handle N
sleep 5; redis-cli -s /var/run/redis/redis.sock -n 0 exists ROUTE_TABLE:198.51.100.0/24 ROUTE_TABLE:Vrf-test:198.51.101.0/24   # 2

# 5. CPU on the e6500 (2 sessions at 300 ms; repeat with step 1's BGP BFD back on for 4)
P=$(pgrep -x bfdd); S=$(systemctl show -p MainPID --value sonic-bfdsyncd)
a=$(awk '{print $14+$15}' /proc/$P/stat); b=$(awk '{print $14+$15}' /proc/$S/stat); sleep 60
echo "bfdd $(( ($(awk '{print $14+$15}' /proc/$P/stat) - a) * 100 / 6000 ))% bfdsyncd $(( ($(awk '{print $14+$15}' /proc/$S/stat) - b) * 100 / 6000 ))%"   # % of one core (CLK_TCK 100)
vtysh -c 'show bfd peers counters' | grep -E 'peer|Session down'    # session down count stays 0 over 10+ min
#   VM: bfdd 0.5 %, bfdsyncd 0.1 % with 4 sessions; expect a few % at most here.
#   If bfdsyncd is visible: services.sonic.bfd.pollInterval = 500 (static routes then react up to 0.5 s later).

# 6. teardown
sonic-db-cli CONFIG_DB del 'STATIC_ROUTE|default|198.51.100.0/24' 'STATIC_ROUTE|Vrf-test|198.51.101.0/24'
sonic-db-cli CONFIG_DB del 'BGP_NEIGHBOR|10.0.0.1' 'BGP_NEIGHBOR|Vrf-test|10.0.0.0'
config interface ip remove Ethernet1/1 10.0.0.0/31; config vrf del Vrf-test
```

On the switch (2026-11-15): as expected for steps 2, 4, 5; step 3 (port shutdown)
logged Down 4.5-4.7 s after its T0, which includes the CLI call, not ~1 s.
Open: IPv6 BFD and multihop untested (bfdd supports both;
staticroutebfd multihop via `bfd.staticRoute.multihop`); bfdsyncd polls
(event-driven would need FRR's zapi or mgmtd notifications).
