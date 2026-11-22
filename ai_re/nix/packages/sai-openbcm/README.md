# sai-openbcm: a thin SAI on the Broadcom OpenBCM SDK

`libsai.so` implementing the OCP Switch Abstraction Interface (SAI 1.19, the
headers sairedis is pinned to) on top of Broadcom's source-available OpenBCM
SDK 6.5.27, so SONiC's `syncd` can drive the BCM56930 (Tomahawk, driver
BCM56960_A0) of the AS7760-32X from its big-endian ppc64 control plane, where
no vendor SAI binary exists. Status: builds natively (fake) and cross
(openbcm); everything above the SDK is unit- and VM-tested on x86_64 with the
fake backend. On the AS7760 it runs `services.sonic` at boot; on the chip
(2026-11-15, one 25G lab loop): ports, VLAN, LAG (LACP under orchagent), port
router interfaces, VRFs, routes via next hops and ECMP groups, ingress ACLs
with counters, CoPP policers, SAI temperatures and the temperature publisher
(evidence: Status of [sonic-nixos PROGRESS](../../../devices/sonic-nixos/PROGRESS.md)).
`openbcm-asic-temp --shutdown-at` has never fired on hardware.

## Design

```
 syncd (sairedis-openbcm)
   | sai_api_query / generic create,get,set,remove (C ABI, SAI 1.19)
 SAI layer  src/sai_*.cpp      object model, OIDs, attribute handling, events
   | ob::Backend (src/backend.h): ~60 operations, SAI status codes
   +-- fake     src/fake_backend.cpp   in-memory, deterministic, no hardware
   +-- openbcm  src/openbcm_backend.cpp -> src/sdk_glue.c -> libsdk.so (BCM API, diag shell, KNET kcom)
```

- **Language: C++17** for the SAI layer and backends (std containers for the
  object model, the same toolchain sairedis/syncd already need; no exceptions
  cross the C ABI: every entry point runs inside `guarded()`), **C** for
  `sdk_glue.c`, the only file that includes SDK headers. It is compiled with
  exactly the defines the SDK was built with (`share/openbcm/cflags` of the
  `libsdk` package: chip, endianness, word size, features), so struct
  layouts match; the C++ side never sees an SDK type.
- **One global context**, one recursive mutex taken by every SAI call.
- **OIDs**: bits 63..56 object type, 55..48 switch index, 47..0 index
  (`src/sai_internal.h`). Ports use the SDK logical port as index, VLANs the
  VID, queues/IPGs `(port << 8) | n`, the rest a per-type counter.
  `sai_object_type_query` / `sai_switch_id_query` only decode.
- **Events**: SDK threads (linkscan, L2X learn/age) and the fake call backend
  callbacks that only queue; a notifier thread takes the API lock, updates the
  model and calls syncd's `PORT_STATE_CHANGE_NOTIFY` / `FDB_EVENT_NOTIFY`
  outside the lock (no lock-order inversion with SDK locks).
- **Stubbed attributes**: a short per-object list of scalar attributes is
  accepted, stored and returned, but not programmed (marked *stub* below).
  Everything else answers `SAI_STATUS_ATTR_NOT_SUPPORTED_0 + index` (or
  `NOT_SUPPORTED` / `NOT_IMPLEMENTED` for whole objects).
  `sai_query_attribute_capability` advertises only real attributes
  (`src/sai_caps.cpp`; a unit test gets every advertised attribute).
- **Errors**: BCM_E_* map to SAI codes in `openbcm_backend.cpp`; attribute
  failures are re-indexed to the failing attribute. Bad input (NULL, wrong
  OID type, unknown attribute) returns an error, never aborts.

## Profile keys (sai.profile, read through the service method table)

| key | default | meaning |
| --- | --- | --- |
| `SAI_OPENBCM_BACKEND` | `openbcm` in SDK builds; **none** in fake-only builds | `openbcm` or `fake`. A fake-only libsai refuses to start without `=fake`; the fake refuses to start when `/dev/linux-user-bde` or `/dev/linux-kernel-bde` exists unless `SAI_OPENBCM_FAKE_ON_HARDWARE=1`. The fake logs a warning on every start. |
| `SAI_INIT_CONFIG_FILE` | (required for openbcm) | SDK `config.bcm`. Missing/unreadable: the switch is not created (the SDK would otherwise init with compiled-in defaults). |
| `SAI_OPENBCM_RC_FILE` | none | diag-shell script run (`rcload`) right after `init all`, before KNET cleanup and before ports are disabled. Missing file: switch create fails. |
| `SAI_OPENBCM_UNIT` | 0 | SDK unit |
| `SAI_OPENBCM_RIF_VLAN_BASE` | 4094 | port router interfaces take internal VLANs counting down from here |
| `SAI_OPENBCM_LOG_STDERR` | 0 | also log to stderr (always syslog, `sai-openbcm` prefix, per-API level from `sai_log_set`, default WARN) |
| `SAI_OPENBCM_TEMP_FILE` / `_PROM` | none | while the switch exists, publish the ASIC temperature to these files, exactly as `openbcm-asic-temp --out/--prom` (same code, `src/temp_files.c`); neither set: no publisher thread |
| `SAI_OPENBCM_TEMP_INTERVAL` | 5 | publisher period in seconds (1..3600) |
| `SAI_OPENBCM_PORT_INTERFACE` | SDK default | board port quirk, every front-panel port: interface type the MAC/SerDes runs (`caui`, `caui4`, `xlaui`, `cr4`, `sr4`, `kr4`, `sfi`, `xfi`, ...: `port_interface_names()` in backend.h), set at switch creation and again after every speed change. For boards whose ASIC lanes end at another chip (retimer, MACsec PHY) instead of a cage. Unknown name, or a quirk the chip refuses: the switch is not created |
| `SAI_OPENBCM_PORT_FEC` | from SAI | `none`/`rs`/`fc`: the ASIC-side FEC is fixed to this; `SAI_PORT_ATTR_FEC_MODE` is accepted, kept and returned, not programmed (the external PHY does the line-side FEC) |
| `SAI_OPENBCM_PORT_AUTONEG` | from SAI | `off`/`on`: fixed the same way; `AUTO_NEG_MODE` is kept, not programmed |
| `SAI_OPENBCM_PORT_IFG` | SDK default (96) | average inter-frame gap in bit times (64..504, multiple of 8; `bcm_port_ifg_set`), e.g. 128 = `CLMAC_TX_CTRL AVERAGE_IPG=16` |
| `SAI_OPENBCM_FAKE_PORTS` / `_LANES` / `_SPEED` | 32 / 4 / 100000 | fake: port count, lanes per port, initial speed |
| `SAI_OPENBCM_FAKE_LINK` | `admin` | fake: link follows admin state; `none`: never (loopback still links) |
| `SAI_OPENBCM_FAKE_NETDEV` | `none` | fake: `tap` makes every hostif netif a Linux TAP netdev of that name (MAC set, carrier = hostif OPER_STATUS), as KNET does on the switch; needs CAP_NET_ADMIN and `/dev/net/tun`. For running swss on the fake. |
| `SAI_OPENBCM_DIAG_SOCKET` | none | debug: unix socket path; syncd then runs SDK diag-shell commands sent to it (one line per connection, output back), under the SAI lock; root (or syncd's uid) only, socket 0600 in a 0700 directory; `exit`/`quit`/`init`/`reset`/`rcload`/`!`..., and `;` chains are refused. Client `bin/openbcm-diag <cmd>` (default socket `/run/sai-openbcm/diag.sock`, `$OPENBCM_DIAG_SOCKET`, `-s`). On in the AS7760 board config |
| `SAI_OPENBCM_FAKE_WIRE` | none | fake, `tap` only: `<netif>:<ifname>,...`; while the port is admin up, frames the TAP sends leave through host interface `<ifname>` and everything `<ifname>` receives goes up the TAP: the CPU path of that port (what KNET carries), no switching between ports. One thread per wire. |

## Safety rules (code + this doc)

- After SDK init (and the rc script), **every front-panel port is disabled**
  (`switch_build_objects`, both backends): SAI's default `ADMIN_STATE` is
  false and SONiC sets it explicitly; the SDK leaves ports enabled after
  `init all`. Ports are only enabled by `SAI_PORT_ATTR_ADMIN_STATE=true`.
- **Default VLAN 1 keeps the SDK's membership** (all front-panel ports
  untagged + CPU) and is reported as 32 VLAN members on 32 default bridge
  ports, which is what orchagent expects to find and remove; we do not strip
  it ourselves.
- **KNET**: stale netifs and filters are destroyed at init (IDs survive SDK
  restarts; a stale filter once delivered frames to the wrong netdev). The
  SDK's own default filter to the RX API is kept.
- No config file, no init. No fake on a host with BDE devices. A config with
  `l3_alpm_enable` is refused while libsdk is built without ALPM (the soc layer
  would lay out the LPM for ALPM and every route add would land where the chip
  never looks: hardware try 2, 2026-11-15).
- **One SDK process per unit**: the SDK boot takes `/run/openbcm-unit<N>.lock`
  (flock, held for the process's life) and fails with BCM_E_BUSY when another
  holder (`openbcm-asic-temp`, the wrapped `bcm.user`, another syncd) has it.
- Tomahawk management ports (`PBMP_MANAGEMENT`, xe0/xe1 in the factory
  configs) are not exposed as SAI ports.
- **Temperature files** (`src/sai_temp.cpp`): written only after a
  successful switch create, removed on switch remove, at `exit()` and after
  a failed or implausible read (outside -40..150 C), so fand sees "no
  sensor" (fans to 100%), never an old reading. The fake never publishes on
  a host with BDE devices. No over-temperature action in SAI mode: "init
  all" armed the SDK's own handler (core PLL off at 125 C).
- **LAG members** carry no L2/L3 state of their own: `create_lag_member`
  fails (`OBJECT_IN_USE`) while the port has a bridge port or router
  interface (orchagent removes the default ones first). A port's hostif and
  KNET netdev are untouched by LAG membership.

## Supported objects and attributes

C/S/G = create/set/get. *stub* = stored and returned, not programmed.

| object | supported | stubbed | not supported |
| --- | --- | --- | --- |
| SWITCH | create (INIT_SWITCH=true only), remove; G: TYPE, PORT_NUMBER, PORT_LIST, CPU_PORT, MAX_NUMBER_OF_SUPPORTED_PORTS, DEFAULT_VIRTUAL_ROUTER_ID, DEFAULT_VLAN_ID, DEFAULT_1Q_BRIDGE_ID, DEFAULT_STP_INST_ID, DEFAULT_TRAP_GROUP, PORT_MAX_MTU, OPER_STATUS, QOS_MAX_NUMBER_OF_TRAFFIC_CLASSES, NUMBER_OF_(UNICAST_)QUEUES, MAX_NUMBER_OF_TEMP_SENSORS, TEMP_LIST / MAX_TEMP / AVERAGE_TEMP (live PVTMON read, whole C rounded); the capacity attributes orchagent reads ([table below](#what-orchagent-reads)): ACL_ENTRY_MINIMUM/MAXIMUM_PRIORITY, TOTAL_BUFFER_SIZE, AVAILABLE_IPV4/IPV6_ROUTE/NEXTHOP/NEIGHBOR_ENTRY, AVAILABLE_FDB_ENTRY, AVAILABLE_NEXT_HOP_GROUP(_MEMBER)_ENTRY (live), NUMBER_OF_ECMP_GROUPS, ECMP_MEMBERS, MAX_ECMP_MEMBER_COUNT (SDK), MAX_ACL_ACTION_COUNT, ACL_STAGE_INGRESS/EGRESS (ingress: PACKET_ACTION, COUNTER, SET_POLICER; egress: none); C/S/G: SRC_MAC_ADDRESS, FDB_AGING_TIME, all *_NOTIFY pointers (port state + FDB events are raised) | ECMP/LAG hash seed/algorithm/offset, RESTART_WARM, WARM_RECOVER, PRE_SHUTDOWN, FAST_API_ENABLE, UNINIT_DATA_PLANE_ON_REMOVAL, VXLAN_DEFAULT_PORT, BCAST/MCAST_CPU_FLOOD_ENABLE, SWITCHING_MODE, FDB_*_MISS_PACKET_ACTION, COUNTER_REFRESH_INTERVAL, SWITCH_ID, SWITCH_PROFILE_ID; HARDWARE_ACCESS_BUS etc. ignored at create; SWITCH_HARDWARE_INFO returns empty | ECMP_HASH/LAG_HASH objects, VXLAN_DEFAULT_ROUTER_MAC, MAX_STP_INSTANCE, ACL resource lists (AVAILABLE_ACL_TABLE*), NAT/IPMC/L2MC/MY_SID available counts, ACL table priority range, switch stats, INIT_SWITCH=false |
| PORT | G: TYPE, HW_LANE_LIST (SDK physical lanes), OPER_STATUS (live), SPEED/OPER_SPEED (live), SUPPORTED_SPEED, SUPPORTED_FEC_MODE, SUPPORTED_AUTO_NEG_MODE, QOS_NUMBER_OF_QUEUES, QOS_QUEUE_LIST, NUMBER_OF_INGRESS_PRIORITY_GROUPS, INGRESS_PRIORITY_GROUP_LIST, QOS_NUMBER_OF_SCHEDULER_GROUPS (8), QOS_SCHEDULER_GROUP_LIST; S/G: ADMIN_STATE, SPEED, MTU (frame max = MTU + 8), FEC_MODE (NONE/RS/FC), AUTO_NEG_MODE, PORT_VLAN_ID, LOOPBACK_MODE (NONE/MAC/PHY), INGRESS_ACL (table group or table; EGRESS_ACL null only), the QoS and mirror attributes (rows below); stats: IF_IN/OUT_{OCTETS,UCAST,NON_UCAST,BROADCAST,MULTICAST,DISCARDS,ERRORS}, IF_IN_UNKNOWN_PROTOS (read; clear-all) | MEDIA_TYPE, INTERFACE_TYPE, DEFAULT_VLAN_PRIORITY, DROP_(UN)TAGGED, LINK_TRAINING_ENABLE, GLOBAL_FLOW_CONTROL_MODE, UPDATE_DSCP, FULL_DUPLEX_MODE, IPG, PKT_TX_ENABLE, HOST_TX_SIGNAL_ENABLE, AUTO_NEG_FEC_MODE_OVERRIDE | create/remove (breakout), ADVERTISED_SPEED, PORT_SERDES, policer bindings, per-counter clear, stats_ext READ_AND_CLEAR |
| QUEUE / INGRESS_PRIORITY_GROUP | 8 + 8 per port, G: TYPE, INDEX, PORT, PARENT_SCHEDULER_NODE (its scheduler group) / PORT, INDEX; S/G: SCHEDULER_PROFILE_ID, WRED_PROFILE_ID, BUFFER_PROFILE_ID / BUFFER_PROFILE; stats (+ _ext READ_AND_CLEAR, clear): what the backend maps | PFC_DLR_INIT | multicast queues |
| QoS (sai_qos.cpp) | QOS_MAP (DSCP/DOT1P_TO_TC/COLOR, TC_TO_QUEUE/PRIORITY_GROUP, PFC_PRIORITY_TO_QUEUE/PG, TC_AND_COLOR_TO_DSCP/DOT1P; complete tables to the backend), SCHEDULER, SCHEDULER_GROUP (fixed: one per queue, level 0, parent = port; S SCHEDULER_PROFILE_ID), WRED, BUFFER_POOL / BUFFER_PROFILE; PORT QOS_*_MAP, QOS_DEFAULT_TC, PRIORITY_FLOW_CONTROL(_MODE/_RX/_TX), QOS_SCHEDULER_PROFILE_ID (shaper), QOS_INGRESS/EGRESS_BUFFER_PROFILE_LIST (kept), PFC_n_RX/TX_PKTS; SWITCH QOS_*_MAP (ports without their own). Programmed only with a backend that has `qos_supported()` (backend.h "QoS"); otherwise kept here | | scheduler group create/remove, multi-level hierarchies |
| HASH, storm control, VLAN flood control (sai_hash.cpp) | SWITCH LAG_HASH / ECMP_HASH (HASH NATIVE_HASH_FIELD_LIST S/G) and *_DEFAULT_HASH_SEED, PORT BROADCAST/MULTICAST/FLOOD_STORM_CONTROL_POLICER_ID, VLAN *_FLOOD_CONTROL_TYPE ALL/NONE, through `hash_set` / `port_storm_set` / `vlan_flood_set` (backend.h); without them: as before (hash and storm attributes unsupported, flood control and seeds kept) | | hash create, L2MC_GROUP flood control |
| Optional extras (backend.h, "Optional L3 / counter / ACL extras") | ROUTER_INTERFACE stats (get / get_ext / clear + stats capability) and ADMIN_V4/V6_STATE, a glean CPU egress (connected subnets, unresolved next hops), PORT stats by any SAI id (+ capability), ACL_RANGE objects, egress ACL tables / groups / PORT EGRESS_ACL / OUT_PORTS, REDIRECT / SET_TC / SET_DSCP / MIRROR table actions, entry change in place and ADMIN_STATE without re-creation, AVAILABLE_ACL_ENTRY; each only with a backend that has it (`fp_supported`, `l3_intf_stat_supported`, ...; sai-teralynx); without: as before | | |
| MIRROR_SESSION, SAMPLEPACKET (sai_mirror.cpp) | LOCAL and ENHANCED_REMOTE (L3 GRE) sessions; PORT INGRESS/EGRESS_MIRROR_SESSION, INGRESS/EGRESS_SAMPLEPACKET_ENABLE; ACL MIRROR_INGRESS/EGRESS actions (FpRule::mirror_*); HOSTIF type GENETLINK + table entries channel GENETLINK. Programmed only with `mirror_supported()` (backend.h); otherwise kept here (ACL mirror actions and GENETLINK refused) | TC, POLICER, CONGESTION_MODE | REMOTE (RSPAN), sampled mirror sessions |
| VLAN | C/R; C/G VLAN_ID; G MEMBER_LIST, STP_INSTANCE (default); the CPU is a tagged member of every VLAN | LEARN_DISABLE, MAX_LEARNED_ADDRESSES, flood control types, MCAST lookup key types, META_DATA, CUSTOM_IGMP_SNOOPING_ENABLE | stats, flood groups, ACL bindings |
| VLAN_MEMBER | C/R/G; VLAN_TAGGING_MODE UNTAGGED/TAGGED (S re-programs) | | PRIORITY_TAGGED |
| BRIDGE | default .1Q only; G TYPE, PORT_LIST | MAX_LEARNED_ADDRESSES, LEARN_DISABLE, flood control | .1D bridges |
| BRIDGE_PORT | type PORT on a PORT or a LAG (VLAN membership and learning applied to every member), C/R/G; S/G ADMIN_STATE (kept), FDB_LEARNING_MODE (HW/DISABLE/DROP/CPU -> bcm_port_learn) | MAX_LEARNED_ADDRESSES, INGRESS/EGRESS_FILTERING, TAGGING_MODE, limit violation action | SUB_PORT, 1D_ROUTER, 1Q_ROUTER, TUNNEL |
| FDB_ENTRY | C/R/S/G on .1Q VLANs (port or trunk destination): TYPE (STATIC/DYNAMIC), BRIDGE_PORT_ID, PACKET_ACTION (FORWARD/DROP); flush by bridge port / VLAN / type; LEARNED/MOVE/AGED notifications from the SDK's L2 callbacks | META_DATA, ALLOW_MAC_MOVE | .1D (bridge bv_id), flush of static-only on hardware (SDK has no static-only delete) |
| STP | default instance G: VLAN_LIST, BRIDGE_ID, PORT_LIST (empty) | | create, STP ports |
| HOSTIF | type NETDEV on a front-panel PORT = KNET netif (TX to the port) + RX filter on the ingress port (LAG members keep theirs: teamd runs on them); C/G NAME, OBJ_ID, TYPE; C/S/G OPER_STATUS (netdev carrier via /proc/bcm/knet/link), VLAN_TAG (STRIP/KEEP/ORIGINAL: filter re-created), QUEUE (kept) | | FD, GENETLINK, LAG/VLAN objects, packet RX/TX API |
| HOSTIF_TRAP_GROUP | default + C/R/S/G: ADMIN_STATE, QUEUE (kept), POLICER (CoPP field entries for the group's traps, [below](#acl-and-copp-field-processor)) | | |
| HOSTIF_TRAP | C/R/S/G TRAP_TYPE, PACKET_ACTION, TRAP_PRIORITY, TRAP_GROUP. Programmed: ARP request/response, DHCP, IGMP, MLD, IPv6 ND (switch controls); LACP, LLDP (exact L2 user entry, BPDU flag: CPU only, or discard when not trapped; removed: discard) | all other trap types are kept but **not programmed** (BGP, ... rely on SDK defaults / routes to CPU) | user-defined traps |
| HOSTIF_TABLE_ENTRY | C/R/G (kept): WILDCARD + NETDEV_PHYSICAL/LOGICAL_PORT is what the per-port KNET filters already do | other types/channels kept, not programmed | set |
| VIRTUAL_ROUTER | SDK VRF id (default VR = VRF 0, others 1..`l3info_max_vrf`); C/R; C/S/G SRC_MAC_ADDRESS (default for RIFs created without one), ADMIN_V4/V6_STATE (true only) | VIOLATION_TTL1/IP_OPTIONS_PACKET_ACTION, UNKNOWN_L3_MULTICAST_PACKET_ACTION | ADMIN_V4/V6_STATE false, labels |
| ROUTER_INTERFACE (M2) | in any VR (the L3 interface binds its VLAN to the VR's VRF); PORT on a port or a LAG (internal VLAN from 4094 down, port / every member untagged + PVID, L3 intf; neighbors egress to the trunk), VLAN, LOOPBACK (no SDK object); C/S/G SRC_MAC_ADDRESS (my-station entries), MTU; ADMIN_V4/V6_STATE kept | NEIGHBOR_MISS_PACKET_ACTION, V4/V6_MCAST_ENABLE, LOOPBACK_PACKET_ACTION, IS_VIRTUAL, NAT_ZONE_ID, DISABLE_DECREMENT_TTL, ADMIN_MPLS_STATE | SUB_PORT, BRIDGE, MPLS, stats |
| NEIGHBOR_ENTRY (M2) | host entry in the RIF's VRF; C/R/G DST_MAC_ADDRESS (S: egress replace), PACKET_ACTION FORWARD/DROP/TRAP, NO_HOST_ROUTE; egress port from the port RIF, or the FDB for VLAN RIFs (CPU until known) | | remove_all, encap index |
| NEXT_HOP (M2) | type IP, C/R/G; uses the neighbor's egress object (CPU until the neighbor exists, then routes are re-pointed) | | other types |
| ROUTE_ENTRY (M2) | any VR (route in its VRF), IPv4/IPv6, C/R/S/G PACKET_ACTION (FORWARD/DROP/TRAP), NEXT_HOP_ID = next hop (may sit in another VR: route leak), next-hop group (ECMP group; drop while it has no resolved member), RIF (connected: to CPU), CPU port (IP2ME) | META_DATA | |
| NEXT_HOP_GROUP | type ECMP: SDK ECMP group (`bcm_l3_egress_ecmp_create`, routes BCM_L3_MULTIPATH) over the egress objects of the members' resolved next hops, created with the first resolved member, updated in place, destroyed with the last; C/R; G TYPE, NEXT_HOP_COUNT, NEXT_HOP_MEMBER_LIST | | other types (ordered, fine-grain, protection, ...), counters, bulk APIs |
| NEXT_HOP_GROUP_MEMBER | C/R/G NEXT_HOP_GROUP_ID; C/S/G NEXT_HOP_ID, WEIGHT (repetitions of the egress object; more paths than the chip's group size are cut, with a warning); unresolved next hops are kept and join when their neighbor appears | SEQUENCE_ID | CONFIGURED/OBSERVED_ROLE, MONITORED_OBJECT, bulk create/remove |
| LAG | SDK trunk (bcm_trunk_*, PSC RTAG 6 = src+dst IP); C/R; G PORT_LIST (LAG_MEMBER ids); C/S/G PORT_VLAN_ID (every member), INGRESS_ACL (applies to every member, follows membership; EGRESS_ACL null only) | DEFAULT_VLAN_PRIORITY, DROP_(UN)TAGGED, TPID | SYSTEM_PORT_AGGREGATE_ID, hash/ARS, labels, sampling, ACLs |
| LAG_MEMBER | C/R/G LAG_ID, PORT_ID; C/S/G EGRESS_DISABLE, INGRESS_DISABLE (BCM_TRUNK_MEMBER_*); a joining port takes the LAG's VLANs, PVID, learning and RIF VLAN, a leaving one drops them (PVID 1, learning HW) | WEIGHT | bulk create/remove |
| POLICER | SDK meter (`bcm_policer_create`, one per SAI policer, shared by its field entries); C/R/S/G: METER_TYPE PACKETS/BYTES, MODE SR_TCM/TR_TCM, COLOR_SOURCE (create-only), CIR, CBS, PIR, PBS, GREEN/YELLOW/RED_PACKET_ACTION (FORWARD/DROP) | | STORM_CONTROL, stats (sai_query_stats_capability: none), counter action lists |
| ACL_TABLE | ingress only, bind points PORT/LAG; one SDK field group (auto width, qualifiers = the table's match fields + in-ports); C/R/G: ACL_STAGE, ACL_BIND_POINT_TYPE_LIST, ACL_ACTION_TYPE_LIST, SIZE (kept), match fields SRC/DST_IP(V6), L4_SRC/DST_PORT, IP_PROTOCOL, IPV6_NEXT_HEADER, ETHER_TYPE, TCP_FLAGS, ICMP(V6)_TYPE/CODE, DSCP, ECN, TOS, ACL_IP_TYPE, SRC/DST_MAC, OUTER_VLAN_ID, TTL, IN_PORT(S); G ENTRY_LIST | | egress, ranges (ACL_RANGE: NOT_SUPPORTED), other fields, AVAILABLE_ACL_ENTRY/COUNTER (CRM: unsupported) |
| ACL_ENTRY | one SDK field entry on the ports the table is bound to (none: not installed); C/R/S/G: PRIORITY (SDK entry priority), ADMIN_STATE, the table's match fields, ACTION_PACKET_ACTION (DROP/DENY, FORWARD/TRANSIT = drop cancel, COPY/LOG, TRAP), ACTION_COUNTER, ACTION_SET_POLICER (colour drops) | | redirect, mirror, set-TC/DSCP, every other action |
| ACL_COUNTER | packets + bytes of its entry's SDK counter (carried over when the entry is re-created); C/R/G TABLE_ID, ENABLE_PACKET/BYTE_COUNT; C/S/G PACKETS, BYTES (set = from now on read this value) | | one counter shared by several entries |
| ACL_TABLE_GROUP / _MEMBER | PARALLEL (every SDK group is looked up); C/R/G STAGE (ingress), BIND_POINT_TYPE_LIST (kept, not checked), TYPE, MEMBER_LIST / GROUP_ID, TABLE_ID, PRIORITY (kept) | | SEQUENTIAL, chain groups |
| everything else (QoS maps, schedulers, buffers, WRED, mirror, tunnels, ...) | | | `sai_api_query` -> NOT_IMPLEMENTED |

Global: `sai_api_initialize/query/uninitialize`, `sai_log_set`,
`sai_object_type_query`, `sai_switch_id_query`, `sai_query_api_version`
(1.19.2), `sai_dbg_generate_dump` (text dump of the object model),
`sai_query_attribute_capability`, `sai_query_attribute_enum_values_capability`
(PORT FEC_MODE/LOOPBACK_MODE, HOSTIF TYPE, RIF TYPE, ACL_TABLE_GROUP TYPE, ACL_ENTRY
ACTION_PACKET_ACTION, ACL_TABLE ACL_STAGE, POLICER MODE/METER_TYPE), `sai_query_stats_capability`
(PORT). `sai_object_type_get_availability`, `sai_get_maximum_attribute_count`,
`sai_get_object_count`, `sai_get_object_key`: NOT_SUPPORTED. The bulk-stats /
`query_stats_st_capability` / TAM entry points are deliberately absent, so
syncd's configure leaves them out (VendorSai then uses its null paths).

## ACL and CoPP (field processor)

[src/sai_acl.cpp](src/sai_acl.cpp); backend: `fp_group_*`, `fp_entry_*`,
`policer_*` (fake: in-memory with Tomahawk-like limits, 12 groups; openbcm:
`bcm_field_*` / `bcm_policer_*` in `sdk_glue.c`). All in the ingress field
processor (IFP); every SDK group is looked up, so SAI table groups are
PARALLEL.

- **ACL**: table = SDK field group (priority = creation order, below CoPP),
  entry = SDK field entry with in-ports = the ports its table is bound to
  (PORT/LAG INGRESS_ACL -> table group -> member tables; LAG: its member
  ports, re-programmed when membership changes). An unbound or admin-down
  entry is not installed. Any change re-creates the SDK entry; its counter
  value is folded into the SAI counter first.
- **CoPP**: a trap group's POLICER meters its traps' CPU-bound packets with
  entries in one CoPP field group (priority 1000), the SAI policer's SDK
  meter attached to all of them (one shared bucket). Matches: ARP
  request/reply (IpType), ND/MLD (ICMPv6 type), DHCP/DHCPv6 (UDP port), IGMP,
  LACP/LLDP/EAPOL (EtherType), STP (BPDU MAC), and, qualified with
  "routed to the CPU egress object" (`DstL3Egress`): BGP/BGPv6, SSH, SNMP,
  BFD (L4 port, priority 150) and IP2ME (everything else to the CPU,
  priority 100; neighbor-miss glean traffic also lands here). Red (yellow,
  green) packets per the policer's actions: for traps the CPU gets as a copy
  (switch controls: ARP, ND, DHCP, IGMP, MLD) only the copy is cancelled
  (`RpCopyToCpuCancel`), the others are dropped (`RpDrop`). Trap types
  without a match (TTL error, UDLD, sample packet, NAT, ...) are not policed
  (INFO log). syncd logs `CoPP: N field entries, P policers`.
- Default SONiC CoPP (`copp_cfg.json`): every group has an sr_tcm packet
  policer (CoppOrch creates one per group it installs: 6 in the VM). ARP + ND
  share 600 pps, LACP + BGP 6000, LLDP 100, DHCP 100, IP2ME 6000. VM (no
  BGP/LLDP traps): 9 CoPP entries on 3 policers.
- ACL counters reach COUNTERS_DB only with `counterpoll acl enable` (the
  ACL_STAT_COUNTER group is off unless `services.sonic.flexCounters` seeds it).
- AclOrch adds FIELD_ACL_RANGE_TYPE (L4 port ranges) to every ingress table:
  accepted and kept, but ACL_RANGE objects are NOT_SUPPORTED, so a rule with
  `L4_SRC/DST_PORT_RANGE` fails (single ports work).

## What orchagent reads

swss pin `d9684cc` (nix/packages/sonic/sources.json). orchagent aborts its
init only on a failed get of the ACL entry priority range (AclOrch throws);
everything else below is logged and survived, but the answer decides what
orchagent does later. Values: openbcm = SDK at run time, fake = constants in
`fake_backend.cpp` (Tomahawk values for the fixed ones).

| SWITCH attribute | answer | orchagent path (why this answer) |
| --- | --- | --- |
| ACL_ENTRY_MINIMUM/MAXIMUM_PRIORITY | openbcm: BCM_FIELD_ENTRY_PRIO_LOWEST..HIGHEST (0..0x7fffffff); fake: same | AclOrch::init, fatal if the get fails |
| MAX_ACL_ACTION_COUNT, ACL_STAGE_INGRESS/EGRESS | 3; ingress PACKET_ACTION, COUNTER, SET_POLICER (list not mandatory), egress none | queryAclActionCapability -> STATE_DB ACL_STAGE_CAPABILITY_TABLE; rules with other actions are refused by AclOrch; MAC-move guard (needs SET_DO_NOT_LEARN) soft-disables itself |
| ACL table match fields (attribute capability) | the fields in the ACL_TABLE row (create) | AclTableTypeBuilder keeps only these in its table types (L3/L3V6 keep all theirs; MCLAG loses OUT_PORTS, PFCWD/DROP lose TC) |
| NUMBER_OF_ECMP_GROUPS, ECMP_MEMBERS, MAX_ECMP_MEMBER_COUNT | openbcm: bcm_l3_info max ECMP groups / paths per group, L3_ECMP table entries; fake: 4096 / 64 / 16384 | RouteOrch caps its groups at NUMBER_OF_ECMP_GROUPS (beyond: a multipath route on one next hop) and logs MAX_ECMP_MEMBER_COUNT. All three go to syncd's log at init (openbcm) |
| AVAILABLE_NEXT_HOP_GROUP(_MEMBER)_ENTRY | openbcm: bcmSwitchObjectEcmpMax - EcmpCurrent; L3_ECMP entries without a reference in the SDK's L3 software table (what a group reserves, not only its members); fake: capacity minus groups / member entries | CRM |
| TOTAL_BUFFER_SIZE | openbcm: `_TH_MMU_TOTAL_CELLS` x `_TH_MMU_BYTES_PER_CELL` (16384 KB, Tomahawk only, else not supported); fake: same | BufferOrch: STATE_DB BUFFER_MAX_PARAM_TABLE mmu_size, read by buffermgrd (not run). No buffer pools/profiles are implemented |
| AVAILABLE_IPV4/IPV6_ROUTE_ENTRY | openbcm: bcmSwitchObjectL3RouteV4RoutesFree / V6Routes64bFree; fake: 16384 / 8192 minus routes | CrmOrch counters |
| AVAILABLE_IPV4/IPV6_NEXTHOP_ENTRY | free L3 egress objects (bcm_l3_info max - used nexthop); fake: 32768 minus egress | CRM. A next hop uses its neighbor's egress object |
| AVAILABLE_IPV4/IPV6_NEIGHBOR_ENTRY | min(free host entries (IPv6: half; the SDK counts an IPv6 host as 2), free egress); host: bcm_l3_info max_host - occupied_host; fake: 16384 hosts | CRM |
| AVAILABLE_FDB_ENTRY | bcmSwitchObjectL2EntryMax - L2EntryCurrent; fake: 32768 minus L2 | CRM |
| MAX_STP_INSTANCE | not supported | StpOrch logs "initialization failure" and has no default STP instance (STP is not implemented; stpmgrd is not run) |
| AVAILABLE_SNAT/DNAT_ENTRY, ECMP_HASH, LAG_HASH, hash algorithm enum capabilities, ACL resource lists | not supported | NAT off, hash objects absent (trunk hash fixed, RTAG 6), CRM marks the resource unsupported |
| HOSTIF QUEUE, queue stats (WRED), trap-type enum capability, policer stats | not advertised / not supported | warnings only; CoppOrch falls back to its default trap list, COPP policer counters off |

## Building and testing (no hardware)

```sh
cd ai_re   # all commands with NIXPKGS_ALLOW_UNFREE=1 for the SDK parts
N='let f = builtins.getFlake "path:'$PWD'/nix"; p = import f.inputs.nixpkgs { system = "x86_64-linux"; };
   sonic = import '$PWD'/nix/packages/sonic { pkgs = p; }; libsai = p.callPackage '$PWD'/nix/packages/sai-openbcm { inherit sonic; }; in'
nix build --impure --expr "$N libsai"                      # fake-only libsai + unit tests (checkPhase)
nix build --impure --expr "$N sonic.sairedis-openbcm libsai"
nix build --impure --expr "$N import $PWD/nix/packages/sai-openbcm/test-syncd.nix { pkgs = p; inherit sonic; }"   # VM test (needs kvm)
# services.sonic (orchagent and all) on the fake, see its header:
nix build -L --impure --expr "$N import $PWD/nix/modules/switch/sonic/tests/vm-openbcm-fake.nix { pkgs = p; inherit sonic libsai; }"
# CoPP policers + an L3 ACL through orchagent on the fake (vm-openbcm-fake-acl.nix, same args)

X='let f = builtins.getFlake "path:'$PWD'/nix"; s = f.nixosConfigurations.as7760; pk = s.pkgs;
   ob = import '$PWD'/nix/packages/openbcm { pkgs = pk; }; sonic = import '$PWD'/nix/packages/sonic { pkgs = pk; };
   libsai = pk.callPackage '$PWD'/nix/packages/sai-openbcm { inherit sonic; libsdk = ob.libsdk { }; }; in'
NIXPKGS_ALLOW_UNFREE=1 nix build --impure --expr "$X ob.libsdk { }"
NIXPKGS_ALLOW_UNFREE=1 nix build --impure --expr "$X libsai"
NIXPKGS_ALLOW_UNFREE=1 nix build --impure --expr "$X sonic.sairedis-openbcm libsai"
```

(`NIXPKGS_ALLOW_UNFREE=1` is needed because the as7760 config only
allow-lists `openbcm-bcm-user`.) Outside Nix: `make SAI_INC=<SAI/inc>
CXXFLAGS=-I<SAI/experimental> check`.

- `tests/test_sai.cpp` (unit, fake backend): OID encoding and status-code
  indexing, init/backend selection (fake never implicit), switch defaults and
  list overflow semantics, port attributes and admin-down safety rule, port
  notifications (admin up -> link up; injected link down), stats, VLAN 1
  default membership and removal, VLAN/bridge-port/member semantics, FDB
  static/dynamic/flush and learn/age notifications, hostif/KNET mapping,
  traps, L3 (port RIF internal VLAN, next hop before neighbor, IP2ME, default
  drop, prefix canonicalisation, in-use checks), VRFs (VR create/remove, RIF
  VRF binding and MAC default, the same neighbor and prefix in two VRFs, IP2ME
  and route leaking, VRF id exhaustion), ECMP (group lifecycle, route on an
  empty group drops, unresolved members join with their neighbor, weights,
  path limit, member next-hop change, in-use checks, free counts), the diag
  socket (permissions, one command per connection, refused commands), LAG (members and disable
  flags, PVID, bridge port + VLAN + FDB on the trunk, join/leave, port RIF +
  neighbor on the trunk, LACP trap, member hostifs), temperature attributes
  and rounding, the temperature publisher (files, failed/implausible read,
  removal), capability table, capacity attributes (values, free counts
  following the model, the still-unsupported ones), fake NETDEV/WIRE profile
  validation, dump, remove/re-create.
- `test_port_quirks` (in test_sai): the `SAI_OPENBCM_PORT_*` keys: applied to every port at creation and after a
  speed change, FEC / autoneg kept but not programmed, bad values and a refused quirk fail the create, no key = no call.
- `tests/test_mdio_script.c`: the MDIO script interpreter on a fake bus (order, polls, failures, refused files).
- `tests/test_acl.inc` (in test_sai): policer attributes, CoPP rules per trap
  type / action / group (shared meter, colour actions, IP2ME priority), ACL
  table qualifier sets, entries installed only when bound, PORT / direct /
  LAG binding following membership, counters carried over re-creation,
  in-use checks, every advertised ACL / POLICER get.
- `test-syncd.nix` (NixOS VM): redis on `/var/run/redis/redis.sock`, syncd
  (`-SUu -z redis_sync`) on the fake, `tests/sai_smoke.py` (the
  sai_smoke.py calls with assertions, plus admin-up of port 0), then ASIC_DB:
  1 switch, 33 ports, 256 queues, 256 IPGs, VLANs 1+100, 32 VLAN members, 32
  bridge ports, 1 router interface, and a `port_state_change` notification.

## Running syncd by hand on the AS7760

Normally `services.sonic` runs syncd (board `as7760`). By hand, for SAI-level
tests (`tests/sai_smoke.py`, sonic-nixos `sai_lag_temp_smoke.py`): the openbcm
kmods, the cross-built `sairedis-openbcm` closure (it pulls in sai-openbcm and
libsdk), redis, the swss-common `database_config.json`, and an SDK config: the
generated one (`SAI_INIT_CONFIG_FILE` in the system's `sai.profile` under
`/usr/share/sonic`) or a factory file (not in this repo).

1. Nothing else may own the ASIC: `systemctl stop sonic.target`, then
   `systemctl stop openbcm-asic-temp` (restartOnStop starts it 10 s after
   syncd stops; fand goes to 100% until syncd's publisher writes the file, see
   step 3), no `bcm.user` running. On the
   NixOS system the modules and nodes are already there; by hand:
   `insmod linux-kernel-bde.ko himem=0 maxpayload=128 dmasize=16M`,
   `insmod linux-user-bde.ko`, `insmod linux-bcm-knet.ko`;
   `mknod /dev/linux-kernel-bde c 127 0; mknod /dev/linux-user-bde c 126 0; mknod /dev/linux-bcm-knet c 122 0`.
2. Redis on the SONiC socket and the DB map:
   `mkdir -p /var/run/redis/sonic-db; cp <swss-common>/share/swsscommon/sonic-db/database_config.json /var/run/redis/sonic-db/`;
   `redis-server --daemonize yes --port 6379 --unixsocket /var/run/redis/redis.sock --unixsocketperm 777 --save "" --databases 16`.
3. Profile, e.g. `/root/sai-openbcm.profile`:
   ```
   SAI_OPENBCM_BACKEND=openbcm
   SAI_INIT_CONFIG_FILE=/root/config-32x100.bcm
   SAI_OPENBCM_LOG_STDERR=1
   SAI_OPENBCM_TEMP_FILE=/run/asic/temperature
   SAI_OPENBCM_TEMP_PROM=/run/prometheus-textfile/asic.prom
   ```
   (the paths [switch/openbcm.nix](../../modules/switch/openbcm.nix) gives
   openbcm-asic-temp; fand keeps its sensor while syncd runs).
4. syncd in the foreground (logs to the terminal and syslog):
   `cd /root && <sairedis-openbcm>/bin/syncd -SUu -z redis_sync -p /root/sai-openbcm.profile`.
   It waits for the client's switch create.
5. Smoke test from another shell:
   `PYTHONPATH=<sairedis-openbcm>/lib/python3.*/site-packages/sairedis:<swss-common>/lib/python3.*/site-packages EXPECT_LANES0=49,50,51,52 ADMIN_UP_PORT0=0 python3 tests/sai_smoke.py`
   (`EXPECT_LANES0` = the lanes of the first port in that config, empty to
   skip; `EXPECT_SPEED0=` (empty) skips the speed check, e.g. in the 128x25
   mode; keep `ADMIN_UP_PORT0=0` until you want a port enabled).
6. Check: syncd log shows `SDK init: ok: stale KNET netifs N filters M destroyed`,
   `switch created: 32 front-panel ports, backend openbcm` (128 in the
   128x25 mode); in the smoke output `port_list 0 ports 32`, lanes, speed,
   `port0_admin 0 False`, `create_rif 0`; ASIC_DB as in the VM test
   (`redis-cli -s /var/run/redis/redis.sock -n 1 --scan --pattern 'ASIC_STATE:SAI_OBJECT_TYPE_PORT:*' | wc -l` = 33).
   Optional: `sai_dbg_generate_dump` via syncd's `saisdkdump`.
7. Stop: Ctrl-C syncd (the process exits; the SDK and its threads go with it).
   Reset: `redis-cli -s /var/run/redis/redis.sock flushall`; the ports were
   left disabled unless the test enabled them; KNET netifs vanish with the next
   SAI start (cleanup) or `rmmod linux-bcm-knet linux-user-bde linux-kernel-bde`.
   A new syncd re-runs `init all`.

## openbcm-mdio-script: PHYs without an SDK driver

`bin/openbcm-mdio-script --config CONFIG.BCM SCRIPT...` runs MDIO scripts through the switch chip's external MDIO
master, for PHYs between the ASIC and the cages that the SDK cannot drive (first user: the AS7772-32X's BCM82391
MACsec PHYs, whose only driver is a vendor binary: the script is that library's recorded bring-up). It is a
separate process that runs once per boot before syncd, not part of libsai, because the PHYs keep their state
across SDK restarts (inside `create_switch` the 100 s download would be repeated on every syncd start, with
orchagent waiting on the create), and because a failed PHY bring-up should fail its own unit, not the switch.
- **Script format** ([src/mdio_script.h](src/mdio_script.h)): text, one operation per line: `W` write, `D` delay,
  `R` read, `E` read and compare (counted), `P`/`N` poll until a value shows / goes away (fails the script),
  `L` label. Clause 45 only (device 0 is a syntax error). No chip knowledge in the runner: whatever makes the
  script decides what is a poll. `--check` parses only (also in fake builds).
- **SDK bring-up**: `init soc` + `init misc` only (`ob_sdk_init_mdio`). Misc init programs the MDIO clock and hold
  delay (`rate_ext_mdio_divisor`, `mdio_output_delay`); when the config sets `mdio_output_delay` the chip's field
  is read back and a difference refuses all MDIO access. Never use MDIO after a bare `init soc` (AS7772: corrupted
  frames hang its PHYs until a power cycle).
- Every script is parsed completely before the chip is touched; a missing or malformed file writes nothing.
  The process takes the per-unit SDK lock (exit 3 when syncd, bcm.user or openbcm-asic-temp holds it).
- Delays under 20 ms are spun (kernels without high-resolution timers round a sleep up to a tick). About 20 us
  per MDIO access on the T2080: 963,000 accesses + 54 s of delays in 98 s.
- NixOS: `hardware.openbcm.mdioScript` ([switch/openbcm.nix](../../modules/switch/openbcm.nix)), script paths
  outside the store. Tests: `tests/test_mdio_script.c` (fake bus; in `make check`).

## openbcm-asic-temp

`bin/openbcm-asic-temp` (SDK builds only, [src/asic_temp.c](src/asic_temp.c)):
the ASIC die temperature monitors via `ob_sdk_attach` (attach only, no
`init all`) + `ob_sdk_temperatures`. `--once` prints them; `--out FILE
[--prom FILE] [--interval S] [--shutdown-at C]` publishes the hottest (C) and
`switch_asic_*` metrics, removes both on exit or a failed read, exits 4 after
3 failed reads.
On a Tomahawk that nothing initialised since power-on it first programs the
S-bus ring map and PVTMON trim exactly as `soc_tomahawk_chip_reset` does
(without that, S-channel reads time out). Run as `openbcm-asic-temp.service`
by [switch/openbcm.nix](../../modules/switch/openbcm.nix); fand reads its file.
Under syncd the same files come from libsai (`SAI_OPENBCM_TEMP_*`, same writer).

Over-temperature: only "init all" arms the chip's own protection, so the
reader does it in software: `--shutdown-at C` (default 125, the SDK's
`_SOC_TH_DEF_TEMP_MAX`; 0 = off). Any current monitor at or above C on 2
consecutive good reads: a CRITICAL line on stderr, both files removed, the
core PLL powered down exactly as `soc_tomahawk_temperature_intr` does
(`TOP_CORE_PLL0_CTRL_1` ISO_IN=1, PWRON=0, PWRON_LDO=0; Tomahawk only), exit
5. The ASIC then needs a power cycle. Only the PLL step: the SDK handler also
resets the pipelines and powers down the serdes first. Not in `--once`; the
glue refuses after `init all` (SAI mode has the SDK handler). Untested on
hardware.

## Known gaps and risks

- **Hardware coverage** (2026-11-15): one 25G lab loop, both ends on the same
  switch. Not exercised: FEC controls, port speed changes that need FlexPort,
  `bcm_port_resource` vs `SOC_INFO` lanes, 100G cables, line rate.
- **Endianness**: only `sdk_glue.c` converts to SDK types; IPv4 addresses
  are assembled from network-order bytes into host integers (`bcm_ip_t`), MACs
  and IPv6 are byte arrays, so the code is byte-order agnostic. The SDK's own
  BE settings (`SYS_BE_PIO=1 SYS_BE_PACKET=0 SYS_BE_OTHER=1`) are the ones the
  bring-up proved with bcm.user and KNET. SAI and SDK headers are compiled for
  the target, no on-wire structs are exchanged.
- **KNET**: netifs are TX_LOCAL_PORT on the port (bypass switch logic) with the
  port's PVID; RX only for packets the ASIC sends to the CPU, which needs traps
  or CPU VLAN membership. LACP and LLDP are trapped (L2 user entries; "init
  all" already sends 01:80:c2:00:00:0x to the CPU); BGP, BFD, SSH and other
  traffic to the switch's own addresses have no trap of their own and reach
  the CPU through the IP2ME host/route entries (CoPP meters them with field
  entries, above); BGP and BFD work this way on the chip. Packets
  not taken by a KNET filter are dropped by our RX handler (no SAI packet API).
- **LAG**: `INGRESS_DISABLE` is the SDK's `BCM_TRUNK_MEMBER_INGRESS_DISABLE`:
  frames received on that member are handled as from a plain port (not
  dropped, learned on the port, which has no SAI bridge port, so no FDB
  event). Trunk hashing is fixed (RTAG 6); LAG hash attributes are stubs.
  On the chip: LAG + member egress disable through syncd, LACP PortChannels
  under orchagent (2026-11-15); INGRESS_DISABLE not exercised there.
- **orchagent on big-endian**: PortsOrch writes `attr.value.u32` for the
  `sai_uint16_t` PORT_VLAN_ID (port and LAG); sairedis serializes `.u16`,
  which on ppc64 is the upper half, i.e. 0. We reject VLAN 0
  (`INVALID_ATTR_VALUE`); fixed in orchagent by swss patch 0001 (nix/packages/sonic/swss).
- **Threads**: SDK linkscan, counter, L2X and RX threads run inside syncd; our
  callbacks only queue. `remove_switch` unregisters callbacks but cannot
  detach the SDK (no clean detach path); a second create in the same process
  re-runs `init all` only (untested on hardware).
- **DMA**: KNET + SDK RX use the BDE DMA pool (`dmasize=16M` factory value);
  untested under load with syncd.
- **L3 termination** (2026-11-15, after the first hardware try dropped every
  unicast frame to the router MAC): the router MAC is one my-station entry
  (`bcm_l2_station_add`, any VLAN) with IPv4, IPv6 and ARP_RARP termination
  (the ARP reply trap needs the last); SDK init sets `bcmPortControlIP4/IP6`
  on every front-panel port (PORT_TAB V4L3/V6L3_ENABLE); an L3 interface's
  VLAN has IPv4/IPv6 L3 enabled. Not an L2 entry with the L3 bit
  (`BCM_L3_ADD_TO_ARL`): it sits on the CPU port, and PORT_TAB's static-move
  drop would discard every frame with SA = router MAC, which on the
  same-switch loop is all of them. Works on the chip (2026-11-15); debug with
  `openbcm-diag` (sonic-nixos vrf-ecmp-test.md, "debug").
- **VRF / ECMP on hardware** (both work on the chip, 2026-11-15): the VRF binding is the
  SDK's (`l3a_vrf` on the L3 interface: VLAN_TAB VRF_ID or the L3_IIF entry of
  the internal VLAN); ECMP groups use the SDK's default group size
  (`max_paths` 0) and are replaced in place (`BCM_L3_REPLACE|WITH_ID`).
  The hash spread is unseen (on the loop both members egress one port); the free ECMP member count reads the
  SDK's L3 software table (`bcm_int/esw/firebolt.h`), an internal structure.
  ECMP hashing is the SDK default (no hash attributes programmed).
- **Not implemented**: QoS (DSCP/TC maps, TC->queue, schedulers, WRED),
  buffers, egress ACLs, ACL ranges/redirect/mirror, port breakout, warm boot
  (`RESTART_WARM` is only stored), mirror, tunnels, sFlow, STP instances.
  orchagent survives all of them (VM test): it reads the
  [capacities](#what-orchagent-reads) and takes its "unsupported" paths.
  QoS is left out on purpose (owner: low priority): SONiC only programs it
  when CONFIG_DB has QOS tables, which the Nix base does not load.
- **ACL / CoPP on hardware** (2026-11-15, [acl-copp-test.md](../../../devices/sonic-nixos/acl-copp-test.md)):
  an L3 ACL bound to one port drops and counts; IP2ME policing (packet-mode
  meter, `DstL3Egress` = the CPU egress object) holds a 66k pps flood to
  ~110/s at 100 pps. Open: ARP passes its 600 pps cap (818/s; probably the
  flood copy to the CPU, which the CoPP entry does not meter). Not checked
  on the chip: IFP group widths for wider qualifier sets, in-ports on a LAG
  or several ports, and meter sharing between one CoPP group's entries (if the
  SDK refuses, each entry gets its own copy: per-entry, not per-group, rate).
- **pysairedis callbacks**: pysairedis' own notification trampolines build
  Python objects without the GIL and crash when libsairedis calls them from
  its notification thread (seen with Python 3.14). `tests/sai_smoke.py`
  registers a ctypes callback instead (it takes the GIL). Not our bug, but
  any Python test client hits it.
- **Symbols**: `libsdk.so` exports every SDK symbol (including `main` from
  socdiag.o, harmless in a library); a name clash with something else syncd
  loads is possible. `libsai.so` exports only `sai_*`.

## Licence

Our code (this directory): Apache-2.0, like sairedis. The openbcm build links
Broadcom's SDK (`libsdk.so`, Broadcom Switch Software License,
source-available, see the SDK's Legal/LICENSE); the combined `libsai.so` and
anything built on it is **not freely redistributable as a binary**. The
fake-only build has no Broadcom code. No vendor config files are included.
