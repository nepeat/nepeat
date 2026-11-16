# CS8210-32X: our SAI, feature by feature

What `sai-teralynx` (sai-openbcm frontend + IFCS backend, [README](../../nix/packages/sai-teralynx/README.md))
does per SAI object, how far it is verified, and which test covers it. Keep this current: one row
per object/attribute group, edited in place. Status:

- **chip**: programmed and checked on the CS8210 (what was checked is in the row);
- **mock**: backend call sequence checked against the mock libifcs only;
- **frontend**: kept by the frontend (stored, answered), nothing programmed;
- **missing**: NOT_IMPLEMENTED / NOT_SUPPORTED.

Tests: `t_*` = `packages/sai-teralynx/tests/t_<area>.cpp` (make check), `hw:x` = oracle script
`oracle/scripts/x.json` on the box ([oracle](../../nix/packages/sai-teralynx/oracle/README.md)),
`bench` = qemu-ipd `tools/sai_bench.cpp`, `smoke` = `tools/sai_smoke.cpp` (2026-11-16 runs),
`sonic` = under SONiC on the loop ([sai-hw-sonic-20261116.txt](sai-hw-sonic-20261116.txt)).
How isai does an object on this chip: [sai-oracle.md](sai-oracle.md).

## Performance (sai_bench, ms per batch, 2026-11-16)

| operation | first native | now (3 runs) | isai 6.2.0 under qemu | note |
|---|---|---|---|---|
| port get OPER_STATUS x32 | 2001 | 0.15 | 3.48 | the 2001 was one 2 s wait on the temperature read |
| port get_stats x4, x32 ports | 55.2 | 6.8 | 27.9 | 12 counter ids per port instead of all 112 |
| create_switch | 3228 | 3235 | 21322 | SDK node create + enable (2.5 s) |
| remove_switch | 243 | 190 | 13208 | |

One ~400 ms wait per 15 s remains (one temperature monitor read holds the libifcs lock); in the
benchmark it lands on the first call after create_switch (`port set mtu` or `get speed` 400 ms).
Evidence: `analysis/sai-teralynx/bench-20261116b/` (bench3-native-error-{1,2,3} `c5b98b1f`
`e4667ec5` `0586723f`, bench2-statsall `86287165`, bench2-oldpaths `3477e2d7`).

## Switch, ports

| SAI | backend (IFCS) | status | test |
|---|---|---|---|
| SWITCH create/remove, PORT_LIST, CPU_PORT, defaults | init sequence of innovium.user; ports down first and last | chip | t_switch, smoke, bench |
| SWITCH TEMP_LIST / MAX_TEMP / AVERAGE_TEMP | node *_LAST_TEMP, one monitor per read, round-robin, cached 15 s | chip (values 49-51 C) | t_switch, hw:temperature |
| SWITCH AVAILABLE_* (CRM), ACL priority range, ECMP limits | our counts + *_usage_get; node limits | mock | t_switch |
| SWITCH FDB_AGING_TIME | node L2_ENTRY_AGING_INTERVAL (seconds: I) | mock | t_l2 |
| SWITCH PORT_STATE_CHANGE_NOTIFY | link scan events | chip (up/down on the loop) | t_port, hw:traps-hw |
| SWITCH FDB_EVENT_NOTIFY | L2 events | mock | t_l2 |
| SWITCH LAG_HASH / ECMP_HASH objects, NATIVE_HASH_FIELD_LIST, *_DEFAULT_HASH_SEED | `hash_set`: extended field lists on node ENTROPY_SELECT_*, ENTROPY_SEED (one entropy for LAG and ECMP: the last set wins); inner fields refused | chip (ECMP over 2 next hops, 256 flows: DST_IP only 1024:0, 5-tuple + seed 504:520; INNER_SRC_IP refused) | t_hooks, hw:phasec-hw |
| SWITCH remove + create again in one process | a new backend object per switch; ifcs_deinit / ifcs_init again | chip (L3 + 100/100 routed in both rounds) | hw:phasec-recreate |
| SWITCH warm boot, switch stats | | missing | |
| PORT ADMIN_STATE | devport ADMIN_STATE | chip | t_port, smoke |
| PORT OPER_STATUS | LINK_STATUS per get (~5 us); `cached` mode from events | chip | t_port (cache), bench |
| PORT SPEED (get; set = configured only), SUPPORTED_SPEED | devport SPEED | chip (get) | t_port, bench |
| PORT FEC_MODE RS/NONE/FC | devport FEC_MODE (KP/KR choice) | chip (RS, link up) | t_port, hw:traps-hw |
| PORT MTU | sysport MTU = SAI MTU + 22 | chip (set), semantics unverified | t_port, bench |
| PORT AUTO_NEG_MODE, LOOPBACK_MODE, PORT_VLAN_ID | devport AUTO_NEG / LOOPBACK, sysport DEFAULT_CVID | mock | t_port |
| PORT LINK_TRAINING_ENABLE, SERDES (TX taps) | only via SAI_TERALYNX_INIT_CMDS files (vendor tables) | frontend (attribute), chip (files) | smoke |
| PORT stats IF_IN/OUT_{OCTETS,UCAST,NON_UCAST,BCAST,MCAST,DISCARDS,ERRORS} | devport_stats_get, 12 ids | chip (octets/packets matched frames sent) | t_port, hw:traps-hw, bench |
| PORT stats IF_OUT_OCTETS | TX_BYTES_OK, as isai (IF_IN_OCTETS stays RX_BYTES_ALL) | chip (Ethernet0 TX_BYTES_OK 179700 = Ethernet120 RX_BYTES_ALL) | t_counters, hw:acl-hw |
| PORT stats past the frontend's 15: ETHER_STATS_* / ETHER_IN / ETHER_OUT size buckets, OVERSIZE, CRC, FEC correctable / not correctable / symbol errors, PAUSE, PFC_n frames + pause duration, WRED (AQM) drops | isai's mapping, 101 ids (`tl_counters.cpp`, sai-oracle.md) through `port_stats_by_id` (one devport_stats_get per call, all port stat ids go this way); `sai_query_stats_capability(PORT)` lists them | chip (get_port_stats: 200 frames in ETHER_STATS / ETHER_IN 128-255, FEC corrected 37, one IFCS call) | t_counters, t_hooks, hw:phasec-hw |
| PORT stats IP_* / IPV6_* (sysport stats), IN/OUT_DROPPED, discards by drop reason, IF_OUT_QLEN | | missing (isai: sysport counters, its drop counters) | |
| PORT create/remove (breakout), PFC, flow control | | missing / frontend | |

## L2, L3

| SAI | backend (IFCS) | status | test |
|---|---|---|---|
| VLAN, VLAN_MEMBER (tagged/untagged) | L2VNI (STP, flood group, learning on) + one mc_dest per member (isai); the CPU in no VLAN (`SAI_TERALYNX_CPU_FLOOD=on`: the old model) | chip (unknown unicast into VLAN 100 reached no netdev; ARP COPY 50/50 frames, one copy each) | t_l2, hw:l2lag-hw |
| frames on a port in no VLAN | no flood to the CPU any more | chip (50 plain frames: 0 at the netdev; was 50) | hw:l2lag-hw |
| VLAN 1 default membership | SDK default: none listed (0 members, 2026-11-16) | chip | hw:traps-hw |
| FDB_ENTRY static/dynamic, PACKET_ACTION drop (port or LAG) | l2_entry MAC_L2VNI, ENTRY_DEST sysport or LAG handle | chip (accepted, port and LAG; forwarding to them untested: one loop) | t_l2, t_lag, hw:lag-l2, hw:l2lag-hw |
| FDB learn/age events (port, LAG), aging, flush by port / VLAN / all | L2 events (a LAG member's sysport maps to its LAG); node aging interval in seconds; get_all walk | chip (learned on the port / LAG bridge port, aged with 10 s within 33 s; flushes accepted) | t_l2, t_lag, hw:l2lag-hw |
| BRIDGE_PORT learning mode HW/DISABLE | sysport LEARN_ENABLE | mock | t_l2 |
| LAG, LAG_MEMBER, EGRESS/INGRESS_DISABLE | ifcs lag + members; out of the IFCS LAG while both disabled (isai), learning off | chip (teamd LACP PortChannel1 = Ethernet0 / PortChannel2 = Ethernet120: selected + current in 1 s; link down/up with members disabled and re-enabled as orchagent does: back in 1 s; LACPDUs on the member netdevs) | t_lag, hw:lag-l2, hw:l2lag-hw |
| LAG as bridge port (VLAN member, PVID, learning) | LAG handle in the L2VNI + one mc_dest; node MULTICAST_LAG_RESOLUTION_ENABLE before enable; untagged / PVID / learn per member sysport | chip (LAG in VLAN 100, learned on its bridge port; isai 6.2.0 fails here: no resolution) | t_lag, hw:lag-l2, hw:l2lag-hw |
| LAG as port RIF | the internal VLAN holds the LAG (tl_l2); next hop dest = LAG handle (tl_l3 via `LagArea::dest_handle`) | chip (RIF, neighbor, route created; no routed traffic) | t_lag, hw:lag-l2 |
| SWITCH LAG/ECMP hash fields, seed | the SAI hash objects (Switch rows); `SAI_TERALYNX_HASH_FIELDS` / `_SEED` still set them at init | chip (ECMP distribution, Switch rows); LAG distribution untested (single-member LAGs) | t_lag, t_hooks, hw:lag-l2 |
| PORT BROADCAST / MULTICAST / FLOOD_STORM_CONTROL_POLICER_ID | `port_storm_set`: an ingress ACL table of its own, one entry per port and kind (DMAC broadcast / group bit; unknown unicast = unicast DMAC + DESTINATION_MAC_HIT 0), the policer's meter, yellow + red dropped, colour counters (diag `acl`) | chip for broadcast (100 pps, 1000 frames at 1000 pps: green 109, yellow 2, red 889); unknown unicast installed, not exercised | t_hooks, hw:phasec-hw |
| VLAN UNKNOWN_UNICAST / UNKNOWN_MULTICAST / BROADCAST_FLOOD_CONTROL_TYPE ALL / NONE | `vlan_flood_set`: one flood group per L2VNI, so all three NONE = an empty MDG, all ALL = the members'; a mixed state is recorded and programmed once the three agree | chip (accepted); effect not observable on one loop | t_hooks, hw:phasec-hw |
| STP instances | one shared instance, every port sysport FORWARDING (a LAG handle is refused: per member) | frontend | |
| VIRTUAL_ROUTER | L3VNI(vrf+1), TTL-check failures copied to the CPU | chip (VRF leaking both ways, one prefix per VRF to different next hops, isolation, v6) | t_l3, hw:l3-vrf |
| ROUTER_INTERFACE PORT | SVI on the frontend's internal VLAN (isai: L3PORT on the sysport) | chip (v4/v6 routed both ways, Linux hosts ping through it) | t_l3, hw:l3-basic, hw:l3-ns |
| ROUTER_INTERFACE PORT on a LAG | next hops to the LAG handle (LagArea::dest_handle) | chip (SONiC: PortChannel1/2 RIFs in Vrf-a/Vrf-b, BGP + BFD over them, loopback ping, ASIC transit 20/20) | t_l3, t_lag, sonic |
| ROUTER_INTERFACE VLAN; LOOPBACK | SVI on the VLAN's L2VNI; loopback: no object | mock; frontend | t_l3 |
| ROUTER_INTERFACE MTU, SRC_MAC set | intf MTU = IP MTU, TRANSPORT_MAC, in place | chip (MTU 1500: IP 1500 routed, 1501 to the CPU; SRC_MAC set live by orchagent from INTERFACE mac_addr, routing on the new MAC) | t_l3, hw:l3-basic, sonic |
| ROUTER_INTERFACE re-create on the same port | the internal VLAN's learned entries flushed before l2vni_delete (L2 area) | chip (RIF B removed: status 0, no BUSY; re-created, 100/100 routed) | hw:phasec-hw |
| ROUTER_INTERFACE ADMIN_V4/V6_STATE | `l3_intf_admin_set`: intf IPV4/IPV6_UNICAST_ENABLE | chip (v4 off: 0/100 routed, v6 100/100; on again 100/100) | t_hooks, hw:phasec-hw |
| ROUTER_INTERFACE NEIGHBOR_MISS_PACKET_ACTION, LOOPBACK_PACKET_ACTION | | frontend (stored) | |
| ROUTER_INTERFACE stats IN/OUT_PACKETS/OCTETS (get, get_ext read-and-clear, clear; stats capability) | `l3_intf_stats`: before node enable a flex block per direction (4096 counters) and the first L3INTF stats mode the SDK takes (`SAI_TERALYNX_L3INTF_STATS_MODES`, default 4,1,2,3; 0.12.41 refuses ALL and takes IPV4_IPV6_ANY); IN = ingress IPv4 + IPv6, OUT = egress IPv4 + IPv6 | chip (100 routed frames: RIF A IN 100 / 13200 B, RIF B OUT 100 / 13200 B) | t_hooks, hw:phasec-hw |
| ROUTE_ENTRY next hop / ECMP / DROP / default / CPU (IP2ME, glean), v4 + v6 | route_entry IP_DEST_L3VNI; CPU = drop + copy to the IP2ME trap (TTL check profile 1); glean (connected subnet, unresolved next hop) = drop + copy to the glean trap (`l3_egress_glean`) | chip (all, with traffic; glean on the NEIGHBOR_MISS queue: hw:phasec-hw) | t_l3, t_hooks, hw:l3-basic |
| IPv4 with options | L3VNI IPV4_OPTIONS_CTC + IPV4_OPTIONS_DROP (isai) + the system trap enabled at init | chip (IHL 6 frames to a routed prefix: 50/50 to the CPU, none forwarded) | t_l3, hw:phasec-hw |
| NEIGHBOR_ENTRY v4/v6, DST_MAC set; resolution through ARP/ND traps | host route + nexthop, updated in place | chip (incl. kernel ARP/ND via the traps); DROP/TRAP actions mock | t_l3, hw:l3-basic, hw:l3-ns |
| NEXT_HOP (IP), unresolved -> CPU until the neighbor exists | nexthop INTF + DEST_MAC + LOCAL_DESTINATION | chip (glean then routed) | t_l3, hw:l3-basic |
| NEXT_HOP_GROUP (ECMP), members add/remove, WEIGHT | ecmp; weight = nexthop repeated; changes = member_replace (as isai) | chip (50:50 over 256 flows, 3:1 = 72:28) | t_l3, hw:l3-basic, hw:l3-ecmpw |
| TTL 1 / TTL 1 to the switch, L3 MTU exceeded | L3VNI TTL CTC + system traps 17 / 23; TTL profile 1 for IP2ME | chip | t_l3, t_copp, hw:l3-basic |
| Route scale | TCAM 24,576: 24,571 v4 /24 or 12,285 v6 /64 (shared); /32 in the host hash: 92,632; ~48 us per v4 route, 52 us v6, 75 us /32 | chip (isai: the same limits) | hw:l3-scale |
| tunnels, IPMC, L2MC, MPLS, NAT | | missing | |

L2 / LAG chip evidence: [sai-hw-l2lag-20261116.txt](sai-hw-l2lag-20261116.txt).

## Host interface, traps, CoPP

Chip evidence for the trap, ACL and counter rows marked acl-hw / acl-hw2 / hostif-create:
[sai-hw-acl-hostif-counters-20261116.txt](sai-hw-acl-hostif-counters-20261116.txt).

| SAI | backend (IFCS) | status | test |
|---|---|---|---|
| HOSTIF NETDEV (adopt the SDK's netdev; the default since 2026-11-16) | rename inno_0_<sysport>, set MAC | chip (Ethernet0/120 TX and RX; all 32 under SONiC with the system MAC) | smoke, hw:traps-hw |
| HOSTIF NETDEV (create mode, `SAI_TERALYNX_NETDEV=create`) | ifcs_netdev_create | chip: **no netdev** with the deployed init yaml (netdev auto_create): SUCCESS, but only the SDK's inno_0_* exist (re-run on the merged build: same) | t_hostif, hw:hostif-create |
| HOSTIF VLAN_TAG, OPER_STATUS | netdev VLAN_TAG; carrier recorded only | mock | t_hostif |
| HOSTIF_TABLE_ENTRY | wildcard = per-port netdev (default) | frontend | |
| HOSTIF_TRAP_GROUP QUEUE | hostif trap QUEUE_NUM; CPU queue counters ifcs_cpu_queue_stats_get (diag `copp`) | chip (frames land on the group's queue: IP2ME queue 1 400, IGMP/MLD queue 3 300, ACL copies queue 0 250) | t_copp, t_counters, hw:traps-hw, hw:acl-hw |
| HOSTIF_TRAP_GROUP POLICER | acl_meter per IB | chip | t_copp, hw:traps-hw |
| HOSTIF_TRAP_GROUP ADMIN_STATE | down = no entries | mock | |
| HOSTIF_TRAP LLDP, LACP, EAPOL (TRAP) | CoPP ACE: DMAC/ether type, copy + drop | chip (50/50 frames on the peer netdev, ACE counters 50) | t_copp, hw:traps-hw |
| HOSTIF_TRAP ARP_REQUEST (COPY) | ACE: broadcast DMAC + 0x0806, copy | chip (one copy per frame since the CPU left the flood groups: 50/50, in a VLAN and in none) | hw:traps-hw, hw:l2lag-hw |
| HOSTIF_TRAP BGP (TRAP) | ACE: 0x0800 + TCP port 179 + IP2ME trap | chip (TCP SYNs to a /32 IP2ME route, counter 50) | t_copp, hw:traps-hw |
| HOSTIF_TRAP IP2ME | the TRAP routes' hostif trap: queue + ACE on it | chip (counter 50) | t_copp, hw:traps-hw |
| HOSTIF_TRAP TTL_ERROR | system trap 17: QUEUE_NUM + ENABLE + ACE | chip (installed; no TTL=1 frames sent yet) | t_copp |
| HOSTIF_TRAP L3_MTU_ERROR | system trap 23: queue + enable (not policed, as isai) | mock | t_copp |
| HOSTIF_TRAP ARP_RESPONSE, DHCP(V6), IPV6_NEIGHBOR_DISCOVERY, STP, PVRST, UDLD, BGPV6, SSH, SNMP, BFD(V6) | ACE shapes as isai's (sai-oracle.md) | mock | t_copp (BGP shape), traps.json (both SAIs) |
| HOSTIF_TRAP IGMP_TYPE_* (5), IPV6_MLD_V1_V2 / V1_REPORT / V1_DONE, MLD_V2_REPORT | ACE: ether type + protocol + message type in L4 byte 1 (isai's) | chip (query, v3 report, MLD query with and without a hop-by-hop header, v2 report: 50/50 on the netdev, ACE counters; once 42/50 for the IGMP v3 batch) | t_copp, hw:acl-hw |
| HOSTIF_TRAP NEIGHBOR_MISS | a glean hostif trap of its own (group queue) + ACE on it, as isai; glean routes copy to it | chip (unresolved host in a connected subnet: 50/50 to the CPU on the group's queue 6, ACE counter 50) | t_copp, t_hooks, hw:phasec-hw |
| HOSTIF_TRAP OSPF, PIM, VRRP, ISIS, user-defined | | missing (kept, not programmed) | t_copp |
| HOSTIF_TRAP SAMPLEPACKET (+ its group's QUEUE / POLICER) | tl_sflow: system trap 512 MIRROR_TO_CPU ENABLE (DROP: off), as isai; the samples' CPU queue is boot-time (init yaml `traffic_monitor_cpu_queue`, board: 2 = SONiC's sflow group); the group's policer = that CPU queue's token-bucket meter (`cpu_queue_attr_set` METER_*, kbit/s at 84 B per packet, I), only when it is the group's queue. isai's ACE on the sFlow collector (MIRROR_2) is impossible here: 0.12.41 refuses a MIRROR match in any ingress profile | mock; chip: queue 2 placement works, the meter is not chip-tested yet; isai's ACE form broke the CoPP table (reverted) | t_sflow, oracle sflow-police, hw ([evidence](sai-hw-sflow-20261116.txt)) |
| system traps at init | isai's: TTL check + IPv4 options to the CPU, L2 source miss/move + L3 MTU off (before the first netdev or trap; `SAI_TERALYNX_SYSTEM_TRAPS=sdk` keeps the SDK's) | chip (set at init; unknown unicast into a port in no VLAN 0/50 on the netdev, phase A 50/50, together with the L2 flood-group change; IGMP/MLD 0 until trapped) | t_hostif, hw:acl-hw |
| HOSTIF_TRAP PACKET_ACTION DROP / TRAP / COPY, TRAP_PRIORITY, group change | entries re-installed | chip (DROP: 0 frames) | t_copp, hw:traps-hw |
| POLICER packets srTCM, CIR/CBS set, RED drop | acl_meter, 84 B per packet | chip (100 pps policer: 1000 pps for 2 s gave 192 frames, red 1808) | t_copp, hw:traps-hw |
| POLICER CIR 10 pps | meter range check refuses 6720 bit/s | chip: **refused** (minimum unknown) | hw:traps-hw |
| POLICER trTCM / bytes / stats | | mock / missing | |
| ACL_TABLE (ingress), TABLE_GROUP, GROUP_MEMBER, PORT / LAG INGRESS_ACL | one node-scope IFCS table per SAI table (PORT_SHARING profile, MAX_WIDTH by key bits, size 256, priority 12-15), its ACL bound to the entries' ports with acl_port_add (isai's form) | chip (L3 and L3V6 tables on Ethernet120, on both ports) | t_acl, hw:acl-hw |
| ACL_ENTRY fields DST/SRC_IP, IP_PROTOCOL, L4_SRC/DST_PORT, TCP_FLAGS, ACL_IP_TYPE IPV4ANY / ARP_REQUEST, DSCP, ICMP_TYPE/CODE, DST_IPV6 + ICMPV6_TYPE, DST_MAC, IN_PORTS | values + masks in network order; ICMP in the L4 source port (v4) / L4 bytes (v6); IN_PORTS on part of the ports: one ACE per port with INGRESS_L2_INTERFACE | chip (each: matching frames dropped / counted, others not: sai-hw evidence) | t_acl, hw:acl-hw, hw:acl-hw2 |
| ACL_ENTRY fields SRC_IPV6, SRC_MAC, OUTER_VLAN_ID, ECN, TTL, ETHER_TYPE, ACL_IP_TYPE IP / NON_IP / IPV6ANY / ARP / ARP_REPLY | same encodings | mock | t_acl |
| ACL_ENTRY PACKET_ACTION DROP / FORWARD / TRAP / COPY, ACTION_COUNTER, ACTION_SET_POLICER, PRIORITY, set (re-installed) | DROP {1,1,1}; FORWARD = zero DROP; TRAP / COPY = COPY_TO_CPU with a shared ACL hostif trap (queue 0); per-entry acl_counter; meter in the table (red drop); ACE priority 0xffffffff - 2 x priority | chip (routed frames: DROP / TRAP not forwarded, FORWARD / COPY forwarded, TRAP / COPY on the netdev; priority 200 FORWARD beat 100 DROP and lost at 50; 100 pps policer let 89 of 1000 through) | t_acl, hw:acl-hw |
| ACL_COUNTER PACKETS / BYTES | acl_counter_stats_get; entry changes in place (`fp_entry_update`: ACEs rebuilt, the IFCS counter kept) | chip (reads lag ~1 s, but two priority changes right after batches lost nothing: 200, then 300) | t_acl, t_hooks, hw:acl-hw2, hw:phasec-hw |
| ACL_RANGE (L4 src / dst port), entry FIELD_ACL_RANGE_TYPE | one acl_comparator (RANGE, VALUE / HI_VALUE) per range in a comparator group per entry, matched as COMPARATOR; profile SRC/DST_L4_PORT_COMPARATOR | chip (dport 1000-2000 DROP: 1500 / 1999 dropped, 3000 forwarded) | t_hooks, hw:phasec-hw |
| ACL_ENTRY ACTION_SET_TC, ACTION_REDIRECT (next hop; port / LAG mock), ADMIN_STATE without re-creation, ACL_TABLE AVAILABLE_ACL_ENTRY | action SET_TC {all colours, tc}; REDIRECT OBJECT = nexthop / sysport / LAG handle; ace_attr_set ENABLE; table size minus ACEs | chip (TC 5: queue 5 +100; redirect to nh2: 100/100 with its DMAC, TTL 63, only with a route present; admin down / up; 256 -> 252) | t_hooks, hw:phasec-hw |
| ACL EGRESS stage tables + groups, PORT EGRESS_ACL, FIELD_OUT_PORTS | egress profiles / table (priority 6, no port match: isai's egress form; one with EGRESS_SYSPORT was PARAM), the ACL bound to the OUT_PORTS / bound ports; drop / counter actions; an entry on part of the bound ports refused | chip (10.20.4/24 out Ethernet120 DROP: 0/100, 10.20.5/24 100/100) | t_hooks, hw:phasec-hw |
| ACL_ENTRY ACTION_SET_DSCP, ACL_IP_TYPE NON_IPV4 / NON_IPV6, SET_TC on egress | IFCS 0.12.41 refuses SET_DSCP in an action profile ("Action not implemented/supported yet 16") | missing (refused) | t_hooks, t_acl |

Seen on the chip (2026-11-16, before the L2 change): every frame entering a port in no VLAN reached
that port's netdev and COPY traps doubled, because the CPU was in every VLAN's flood group (VLAN 1
included). Fixed by taking the CPU out (L2 rows); the system traps are now set as isai does too
(row "system traps at init"). With both, unknown unicast into a port in no VLAN reached the netdev 0/50
times (acl-hw); which of the two stopped it was not separated (`SAI_TERALYNX_SYSTEM_TRAPS=sdk` would tell).

## Frontend hooks

The phase B requests are served (backend.h, all default to the old behaviour, so the AS7760 is
unchanged in function): `hash_supported` / `hash_set`, `port_storm_set`, `vlan_flood_set`,
`l3_intf_stat_supported` / `l3_intf_stats` (RIF get / get_ext / clear, stats capability),
`l3_intf_admin_set`, `l3_egress_glean`, `port_stat_by_id` / `port_stats_by_id` (+ stats capability),
`fp_supported(FpCap)` with `fp_group_create_ex` (ranges, egress, OUT_PORTS, REDIRECT / SET_TC / SET_DSCP /
MIRROR), `fp_entry_update` (in place, counter kept), `fp_entry_enable`, `fp_group_free`. Chip evidence
for all of them: [sai-hw-integration-20261116.txt](sai-hw-integration-20261116.txt).

## QoS, buffers, mirror, sampling, BFD

Chip evidence: [sai-hw-qos-20261116.txt](sai-hw-qos-20261116.txt), [sai-hw-mirror-20261116.txt](sai-hw-mirror-20261116.txt)
(`hw:qos-hw`; frames from the Ethernet0 netdev, classified on Ethernet120 and routed back out of it, counted
with queue and port counters). isai's IFCS sequences: [sai-oracle.md](sai-oracle.md) "QoS", "Mirror".

| SAI | backend (IFCS) | status | test |
|---|---|---|---|
| QOS_MAP DSCP_TO_TC, DOT1P_TO_TC; PORT QOS_DSCP/DOT1P_TO_TC_MAP; SWITCH QOS_DSCP_TO_TC_MAP | qos_map (64 / 8 entries, tc_dp) + one qos_map_group per (DSCP, DOT1P) pair, sysport QOS_*_TO_TC_DP_MAP; update = qos_map_attr_set | chip (DSCP 10/46 to queues 1/5, map change to queue 3) | t_qos, hw:qos-hw |
| QOS_MAP TC_TO_QUEUE | identity only (the chip's fixed TC = queue; isai programs nothing); other maps NOT_SUPPORTED | chip (identity) | t_qos, hw:qos-hw |
| QOS_MAP TC_TO_PRIORITY_GROUP, PFC_PRIORITY_TO_QUEUE / _PRIORITY_GROUP; DSCP/DOT1P_TO_COLOR (all green) | nothing programmed (isai: the same) | frontend | t_qos |
| QOS_MAP TC_AND_COLOR_TO_DSCP / DOT1P (remarking), colour maps with yellow / red | | missing (NOT_SUPPORTED) | t_qos |
| PORT QOS_DEFAULT_TC | sysport DEFAULT_TC | mock | t_qos |
| PORT PRIORITY_FLOW_CONTROL (_MODE, _RX, _TX) | devport PFC_MODE, PFCn(_TX/_RX)_ENABLE | mock (accepted on the chip; CPU-sent PFC frames never counted, see evidence) | t_qos |
| PORT PFC_n_RX/TX_PKTS | devport RX/TX_FRAMES_PFCn | mock (read on the chip: 0) | t_qos |
| SCHEDULER on a queue's SCHEDULER_GROUP (one group per queue, the hierarchy qosorch looks up) / QUEUE SCHEDULER_PROFILE_ID | queue SCHEDULING_MODE strict / WDRR + WDRR_WEIGHT, GUARANTEED / MAX_RATE (bits/s = SAI bytes/s x 8) | chip (DWRR 10:40 drained 1:3.9 under a shaper; strict served first; queue shaper 50 Mbit/s measured 50.2) | t_qos, hw:qos-hw |
| PORT QOS_SCHEDULER_PROFILE_ID (port shaper) | devport MAX_RATE (bits/s) + MAX_BURST_SIZE | chip (MAX_RATE unit measured: 12500000 = 12.5 Mbit/s; isai passes bytes unscaled) | t_qos, hw:qos-hw |
| WRED (thresholds, probability, ECN mark mode) on QUEUE WRED_PROFILE_ID | aqm_curve_group (cells, floor to 8) + queue AQM_CURVE_GROUP, WRED_ENABLE, ECN_EXTENSION_ENABLE | **missing**: IFCS 0.12.41 refuses WRED_ENABLE / ECN_EXTENSION_ENABLE on these queues (also port down); NOT_SUPPORTED, curve rolled back | t_qos, hw:qos-hw |
| BUFFER_PROFILE on QUEUE BUFFER_PROFILE_ID | queue THRESHOLD_MODE, dynamic factors th+5 / th+1, static limits; RESERVED_SIZE (cells, port flapped as isai) refused by 0.12.41 on the chip ("Attr id(2) is Not valid"): kept at the SDK's, thresholds only | chip (SONiC BUFFER_QUEUE: refusal seen, fallback mock only) | t_qos |
| BUFFER_POOL, PORT QOS_INGRESS/EGRESS_BUFFER_PROFILE_LIST | kept (SDK partitions fixed by the init config; isai writes nothing either) | frontend | t_qos (frontend) |
| BUFFER_PROFILE on INGRESS_PRIORITY_GROUP (xoff / xon headroom) | kept, not programmed: no lossless DTM partition in this init config (isai: NOTFOUND) | missing | t_qos |
| QUEUE stats PACKETS, BYTES, DROPPED_PACKETS, CURR_OCCUPANCY / WATERMARK / SHARED_WATERMARK_BYTES, WRED_ECN_MARKED_* | queue_stats_get (occupancy, watermarks = cells x 224) | chip (counts match frames; occupancy drains at the shaper rate; counters lag seconds) | t_qos, hw:qos-hw |
| QUEUE DROPPED_BYTES; IPG PACKETS / BYTES / XOFF_ROOM; BUFFER_POOL stats | | missing | t_qos |
| IPG DROPPED_PACKETS, CURR_OCCUPANCY / WATERMARK_BYTES | bam_queue_stats_get (unit unverified) | mock | t_qos |
| MIRROR_SESSION LOCAL (SPAN), PORT INGRESS_MIRROR_SESSION | DCDP on the monitor sysport (shared by its sessions) + collector SPAN + set, sysport MIRROR_INGRESS | chip (1000 copies for 1000 frames) | t_mirror, hw:qos-hw |
| PORT EGRESS_MIRROR_SESSION | collector egress, sysport MIRROR_EGRESS | chip for forwarded frames (copies truncated to 180 B: no TRUNCATE on egress collectors); CPU-sent frames are not mirrored | t_mirror, hw:qos-hw |
| MIRROR_SESSION ENHANCED_REMOTE (ERSPAN, L3 GRE) | collector ERSPAN_TYPE_II: MACs, IPs, DSCP = TOS >> 2, TTL | chip (copies + 50 B encapsulation, ingress and egress; headers not captured) | t_mirror, hw:qos-hw |
| MIRROR_SESSION TRUNCATE_SIZE, REMOTE (RSPAN), SAMPLE_RATE > 1, TC, POLICER, two sessions on one port and direction | truncate is on/off (size fixed); the rest refused or kept | missing / frontend | t_mirror |
| ACL entry ACTION_MIRROR_INGRESS / _EGRESS | action MIRROR OBJECT = the session's collector set (`MirrorArea::collector_set`, held per entry and direction); first session only | chip (SPAN to Ethernet0: 100 copies for 100 frames) | t_hooks, hw:phasec-hw |
| SAMPLEPACKET + PORT INGRESS_SAMPLEPACKET_ENABLE, SAMPLE_RATE set | DCDP to the CPU + one sFlow collector / set (isai's tags), sysport SFLOW_INGRESS {set, container 1, rate}; SAI's N passed through (isai); every change also passes the port's rate to tl_sflow | chip (random 1 in N, mean N: 1 in 101.1 over 435k packets at 100, 259 at 256, sweep 1-4096; the old "1 in 64" = the generator's fixed seed after init plus clustered gaps; samples never truncated; live rate change under SONiC) | t_mirror, t_sflow, hw:sflow-rate, hw:sflow-rate2, sonic ([evidence](sai-hw-sflow-20261116.txt)) |
| PORT EGRESS_SAMPLEPACKET_ENABLE | node EGRESS_SFLOW_ENABLE before node enable (`SAI_TERALYNX_EGRESS_SFLOW=on`, board default; the yaml key is ignored by 0.12.41), egress collector + sysport SFLOW_EGRESS, rate to tl_sflow (psample group 2) | chip (1 in 9.2 at rate 10, oif Ethernet120; samples cut to 176 B and reported as 176 B frames); refused while the node attribute is off | t_mirror, t_sflow, hw ([evidence](sai-hw-sflow-20261116.txt)) |
| HOSTIF GENETLINK (psample) + HOSTIF_TABLE_ENTRY TRAP_ID SAMPLEPACKET channel GENETLINK | tl_sflow: the entry makes the ipd driver's sFlow netlink on libifcs' /dev/ipd fd (IPD_SET_PARAMS GRE 0x8818, IPD_CREATE_NETLINK SFLOW, per-port IPD_SET_NETDEV sample rate); with ipd psample=1 (nix ipd-sflow-psample.patch) the driver feeds the kernel's psample groups 1/2; other trap types NOT_SUPPORTED, one sFlow netlink | chip (SONiC copporch + sfloworch + hsflowd -> sflowtool: FLOW in=121 rate=1000 / 100, size 260; psample iif = the port netdev, rate, original size; stock ipd: delivery works, rate 0; patched ipd deployed 2026-11-16 21:12Z). Policing / queue: the SAMPLEPACKET trap row | t_sflow, hw:sflow-genl, sonic |
| BFD offload, TAM, warm boot, debug counters | | missing | |

Not testable on this loop: congestion beyond what a port shaper on the CPU's few Gbit/s creates (all
scheduling checks run under a shaper), PFC (CPU-injected 0x8808 frames are not counted), lossless
headroom, ECN marking (refused by the SDK).

## Frontend requests

- **Trap flow counters** (sflow/metrics agent, 2026-11-16): `FLOW_CNT_TRAP` flex counters (SAI_OBJECT_TYPE_COUNTER on
  HOSTIF_TRAP `COUNTER_ID`) are unanswered, so Grafana "Trapped to the CPU" stays empty on both boxes. Wanted: a
  backend hook `trap_counter(int32_t trap_type, uint64_t& packets, uint64_t& bytes)` (NOT_SUPPORTED by default);
  sai-teralynx would answer from the trap's CoPP colour counters (for SAMPLEPACKET: CPU queue 2's counters).
