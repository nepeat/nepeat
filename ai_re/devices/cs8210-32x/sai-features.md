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
`bench` = qemu-ipd `tools/sai_bench.cpp`, `smoke` = `tools/sai_smoke.cpp` (2026-11-16 runs).
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
| SWITCH hash, warm boot, switch stats | | frontend / missing | |
| PORT ADMIN_STATE | devport ADMIN_STATE | chip | t_port, smoke |
| PORT OPER_STATUS | LINK_STATUS per get (~5 us); `cached` mode from events | chip | t_port (cache), bench |
| PORT SPEED (get; set = configured only), SUPPORTED_SPEED | devport SPEED | chip (get) | t_port, bench |
| PORT FEC_MODE RS/NONE/FC | devport FEC_MODE (KP/KR choice) | chip (RS, link up) | t_port, hw:traps-hw |
| PORT MTU | sysport MTU = SAI MTU + 22 | chip (set), semantics unverified | t_port, bench |
| PORT AUTO_NEG_MODE, LOOPBACK_MODE, PORT_VLAN_ID | devport AUTO_NEG / LOOPBACK, sysport DEFAULT_CVID | mock | t_port |
| PORT LINK_TRAINING_ENABLE, SERDES (TX taps) | only via SAI_TERALYNX_INIT_CMDS files (vendor tables) | frontend (attribute), chip (files) | smoke |
| PORT stats IF_IN/OUT_{OCTETS,UCAST,NON_UCAST,BCAST,MCAST,DISCARDS,ERRORS} | devport_stats_get, 12 ids | chip (octets/packets matched frames sent) | t_port, hw:traps-hw, bench |
| PORT FEC / PFC / queue / PG stats, OUT_OCTETS = TX_BYTES_OK (isai) vs TX_BYTES_ALL (ours) | | missing / open | |
| PORT create/remove (breakout), PFC, flow control | | missing / frontend | |

## L2, L3

| SAI | backend (IFCS) | status | test |
|---|---|---|---|
| VLAN, VLAN_MEMBER (tagged/untagged) | L2VNI + STP + flood group (CPU in every VLAN) | chip (create/remove in bench, no traffic) | t_l2, bench |
| VLAN 1 default membership | SDK default: none listed (0 members, 2026-11-16) | chip | hw:traps-hw |
| FDB_ENTRY static/dynamic, PACKET_ACTION drop | l2_entry MAC_L2VNI | chip (100 static in bench, no traffic) | t_l2, bench |
| FDB flush, learn/age events, aging | get_all walk; L2 events | mock | t_l2 |
| BRIDGE_PORT learning mode HW/DISABLE | sysport LEARN_ENABLE | mock | t_l2 |
| LAG, LAG_MEMBER | | missing (NOT_IMPLEMENTED) | t_lag (refusal) |
| STP instances | one shared instance, all FORWARDING | frontend | |
| VIRTUAL_ROUTER | L3VNI(vrf+1) | chip (bench) | bench |
| ROUTER_INTERFACE PORT | SVI on the frontend's internal VLAN | chip (frames to its MAC routed to the CPU) | t_l3, hw:traps-hw |
| ROUTER_INTERFACE VLAN, LOOPBACK; SRC_MAC / MTU set | intf attr_set | mock | t_l3 |
| ROUTE_ENTRY drop / next hop / ECMP / CPU (IP2ME) | route_entry IP_DEST_L3VNI; CPU = drop + copy to the IP2ME trap | chip (1000 drop routes; IP2ME /32 to CPU with traffic) | t_l3, bench, hw:traps-hw |
| NEIGHBOR_ENTRY, NEXT_HOP | host route + nexthop | mock | t_l3 |
| NEXT_HOP_GROUP (ECMP) | ecmp + members (weight) | mock | t_l3 |
| tunnels, IPMC, L2MC, MPLS, NAT | | missing | |

## Host interface, traps, CoPP

| SAI | backend (IFCS) | status | test |
|---|---|---|---|
| HOSTIF NETDEV (adopt the SDK's netdev) | rename inno_0_<sysport>, set MAC | chip (Ethernet0/120 TX and RX) | smoke, hw:traps-hw |
| HOSTIF NETDEV (create mode) | ifcs_netdev_create | mock | t_hostif |
| HOSTIF VLAN_TAG, OPER_STATUS | netdev VLAN_TAG; carrier recorded only | mock | t_hostif |
| HOSTIF_TABLE_ENTRY | wildcard = per-port netdev (default) | frontend | |
| HOSTIF_TRAP_GROUP QUEUE | hostif trap QUEUE_NUM | chip (set; which CPU queue frames use is unverified) | t_copp, hw:traps-hw |
| HOSTIF_TRAP_GROUP POLICER | acl_meter per IB | chip | t_copp, hw:traps-hw |
| HOSTIF_TRAP_GROUP ADMIN_STATE | down = no entries | mock | |
| HOSTIF_TRAP LLDP, LACP, EAPOL (TRAP) | CoPP ACE: DMAC/ether type, copy + drop | chip (50/50 frames on the peer netdev, ACE counters 50) | t_copp, hw:traps-hw |
| HOSTIF_TRAP ARP_REQUEST (COPY) | ACE: broadcast DMAC + 0x0806, copy | chip, but the CPU gets 2 per frame (copy + the original, see below) | hw:traps-hw |
| HOSTIF_TRAP BGP (TRAP) | ACE: 0x0800 + TCP port 179 + IP2ME trap | chip (TCP SYNs to a /32 IP2ME route, counter 50) | t_copp, hw:traps-hw |
| HOSTIF_TRAP IP2ME | the TRAP routes' hostif trap: queue + ACE on it | chip (counter 50) | t_copp, hw:traps-hw |
| HOSTIF_TRAP TTL_ERROR | system trap 17: QUEUE_NUM + ENABLE + ACE | chip (installed; no TTL=1 frames sent yet) | t_copp |
| HOSTIF_TRAP L3_MTU_ERROR | system trap 23: queue + enable (not policed, as isai) | mock | t_copp |
| HOSTIF_TRAP ARP_RESPONSE, DHCP(V6), IPV6_NEIGHBOR_DISCOVERY, STP, PVRST, UDLD, BGPV6, SSH, SNMP, BFD(V6) | ACE shapes as isai's (sai-oracle.md) | mock | t_copp (BGP shape), traps.json (both SAIs) |
| HOSTIF_TRAP IGMP, MLD, NEIGHBOR_MISS, SAMPLEPACKET, OSPF, PIM, VRRP, ISIS, user-defined | | missing (kept, not programmed) | t_copp (IGMP kept) |
| HOSTIF_TRAP PACKET_ACTION DROP / TRAP / COPY, TRAP_PRIORITY, group change | entries re-installed | chip (DROP: 0 frames) | t_copp, hw:traps-hw |
| POLICER packets srTCM, CIR/CBS set, RED drop | acl_meter, 84 B per packet | chip (100 pps policer: 1000 pps for 2 s gave 192 frames, red 1808) | t_copp, hw:traps-hw |
| POLICER CIR 10 pps | meter range check refuses 6720 bit/s | chip: **refused** (minimum unknown) | hw:traps-hw |
| POLICER trTCM / bytes / stats | | mock / missing | |
| ACL_TABLE / ENTRY / COUNTER / GROUP (SAI ACLs) | backend NOT_IMPLEMENTED (the CoPP table code is the base) | missing | |

Seen on the chip, not yet handled: with adopted netdevs, **every frame entering a port that is in no
VLAN reaches that port's netdev** (unicast to an unknown MAC too; no system trap counted it), and
the CPU is in every VLAN's flood group: so COPY traps double, and unmatched traffic is not
policed. isai sets system traps at init (FWD_L2_SRC_MISS_MOVE and L3MTU off, TTL and IPv4
options on; oracle), we leave the SDK defaults.

## QoS, buffers, mirror, sampling, BFD

| SAI | status |
|---|---|
| QUEUE / INGRESS_PRIORITY_GROUP objects (8 + 8 per port), BUFFER_POOL/PROFILE, SCHEDULER(_GROUP), WRED, QOS_MAP (DSCP/TC/queue), PFC | frontend objects only, nothing programmed (`tl_qos.h` slot) |
| MIRROR_SESSION, SAMPLEPACKET / sFlow | missing (`tl_mirror.h` slot) |
| BFD offload, TAM, warm boot, debug counters | missing |
