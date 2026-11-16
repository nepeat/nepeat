# CS8210-32X: what the isai oracle showed (2026-11-16)

The oracle ([packages/sai-teralynx/oracle](../../nix/packages/sai-teralynx/oracle/README.md)) runs one
SAI script on our SAI (native) and on isai 6.2.0 (under qemu-ipd) with every libifcs call traced,
and diffs them per SAI step. Findings that changed our code; traces are local
(`analysis/oracle/<run>/`, sha256 prefix in brackets).

## Performance (`perf.json`, `temperature.json`)

- **The "62 ms per OPER_STATUS get" was the temperature read.** Each `IFCS_NODE_ATTR_*_LAST_TEMP`
  read takes ~400 ms inside libifcs and holds its API lock; the frontend's temperature publisher
  read all five monitors (2 s) every 5 s, so the first SAI call in each window waited ~2 s (the
  benchmark's 2001 ms / 32 calls was one such wait). `LINK_STATUS` itself costs ~5 us.
  [perf-20261116-034818 `2a8fbd10`, temperature-20261116-035053 `38efd958`]
  Fix: one monitor per read, round-robin, at most every 15 s (`SAI_TERALYNX_TEMP_PERIOD`): one
  ~400 ms wait per 15 s remains (visible in sai_bench as a 400 ms step right after create_switch).
- isai reads `LINK_STATUS` per OPER_STATUS get, and for counters asks `devport_stats_get` for
  exactly the requested ids (4), where we asked the vendor CLI's "all 112" form (8x slower).
  Fix: the 12 mapped ids in one call. isai maps `IF_OUT_OCTETS` to `TX_BYTES_OK`, we to
  `TX_BYTES_ALL` (open).
  [perf-20261116-035259: ours `3f36c84c`, isai `c952804c`, report `efd2de2f`]
- isai's create_switch: 4794 IFCS calls (ours 174), 18.8 s of IFCS time under qemu.

## Host traps and CoPP (`traps.json`: SONiC's copp_cfg groups, policers, 17 trap types)

[traps-20261116-035604: isai `455e5bd4`, ours `92514314`, report `9dd757cf`]
- One ingress ACL table (match profile: DMAC, ether type, L4 protocol, L4 ports, L4 byte 1,
  hostif trap, ...; action profile: drop, copy to CPU, copy override, meter, counter, ...),
  scope per inno block, **6 ACLs (IB 0-5)** added to it.
- Per SAI trap: a hostif trap object (isai pre-creates ~40 at init) whose `QUEUE_NUM` is the trap
  group's queue; per IB an ACE: matches (LLDP: three DMACs 01:80:c2:00:00:0e/03/00 + 0x88cc;
  BGP: 0x0800 + port 179 + `HOSTIF_TRAP` = the IP2ME trap; IP2ME / NEIGHBOR_MISS / TTL: the
  hostif trap alone), actions COPY_TO_CPU (that trap; green/yellow, red too when unpoliced),
  COPY_TO_CPU_OVERRIDE (red, when policed), DROP (TRAP action), METER, counter.
- TTL_ERROR = system trap `FWD_L3_TTL_CHECK_FAIL` (17): `QUEUE_NUM` + `ENABLE`, plus an ACE on
  it for the policer; L3_MTU_ERROR = system trap 23, queue + enable only (no policer).
- Policers: `acl_meter` per IB, byte type, RFC 2697, colour aware; pps converted at 84 B per
  packet (CIR = pps x 672, CBS = packets x 84).
- ACE priority: almost all traps 0x7ffffffe, IP2ME 0x80000000, a PFC drop 1: lower wins.
- Our 0.12.41 SDK refuses isai's table attribute `TYPE` and needs `DEFAULT_STATS` off with
  `MAX_COUNTERS`, and `MAX_METERS` set (SDK log); otherwise the same objects work
  ([sai-features.md](sai-features.md), "Host traps").

## QoS (`qos.json`: maps, schedulers, WRED, buffers, PFC, counters on Ethernet0)

[qos-20261116-044410: isai `9d627e77`; port admin up, else isai defers queue writes]
- Hierarchy: 8 scheduler groups per port, one queue each (CHILD_COUNT 1, LEVEL 0, no parent);
  queues TYPE UNICAST, PARENT_SCHEDULER_NODE = the port. No IFCS calls for any of it.
- DSCP / DOT1P -> TC: `qos_map_create(QOS_MAP_INFO {type DSCP_TO_TC_DP | DOT1P_CFI_TO_TC_DP, list})`
  (64 / 8 entries, value tc_dp, dp 0), a `qos_map_group(ACTION_TYPE MAP, DSCP_TO_TC_DP, DOT1P_CFI_TO_TC_DP)`,
  sysport `QOS_DSCP_TO_TC_DP_MAP` / `QOS_DOT1P_CFI_TO_TC_DP_MAP` = the group; a map change is one
  `qos_map_attr_set`. DEFAULT_TC: sysport `DEFAULT_TC`.
- TC -> queue (identity and a 1<->2 swap), TC -> PG, PFC priority -> queue / PG: **no IFCS calls**.
- PFC (0x18): devport `PFC0..7_ENABLE` after reading `PFC_MODE`.
- Scheduler on a queue's group: `queue_attr_set({devport, queue}, SCHEDULING_MODE 1, WEIGHT w,
  GUARANTEED_RATE_ENABLE 0, MAX_RATE_ENABLE 0)`; strict = mode 0; unset = WDRR weight 1; a weight
  change rewrites every queue using it. Port shaper: devport `MAX_RATE_ENABLE 1, MAX_RATE` (SAI's
  bytes/s unscaled: 125000000), `MAX_BURST_SIZE` (65536).
- WRED: `aqm_curve_group_create` with green/yellow/red min, max, maxp; thresholds in 224 B cells
  rounded down to 8 (100000 B -> 440, 2000000 -> 8928), unused colours min = max = 45728, 100 %;
  queue `AQM_CURVE_GROUP`, then `AQM_QSIZE_AVERAGING_WEIGHT, ECN_EXTENSION_ENABLE, WRED_ENABLE`.
- Buffer pools: nothing written (reads IB / DTM partitions). Egress queue profile: `RESERVED_SIZE`
  (cells, 1518 B -> 7) with the port **disabled and re-enabled**, `THRESHOLD_MODE 1`, unicast /
  non-unicast dynamic factor = th + 5 / th + 1. Ingress PG profile: looks for DTM partition 1
  (lossless), NOTFOUND, programs nothing.
- Counters: queue `PACKET_TX_COUNT, BYTE_TX_COUNT, PACKET_DROP_COUNT, BYTE_DROP_COUNT`, watermarks;
  PG counters: no IFCS call (zeros); buffer pool counters: partition stats, NOTFOUND.

## Mirror, sFlow (`mirror.json`, `sflow-genetlink.json`)

[mirror-20261116-044722 `df4af932`, sflow-genetlink-20261116-045019 `b4383ca7`]
- SPAN: `dup_copy_dest_port_create(SYSPORT = monitor)`; per port direction a
  `collector_create(TYPE SPAN, DIRECTION, DCDP[, TRUNCATE])` in a `collector_set(TYPE, DIRECTION)`;
  sysport `MIRROR_INGRESS` / `MIRROR_EGRESS` = traffic-monitor policy {ENABLE, set}; off = {DISABLE,
  the same set}, then the set goes. A session change re-creates the collector inside the same set.
- ERSPAN: collector `ERSPAN_TYPE_II` with DEST/SRC MAC, SRC/DEST IP, DSCP = TOS >> 2, TTL, SESSION_ID 0;
  no GRE type, no VLAN attributes. Truncation is on/off (TRUNCATE_SIZE reads 192).
- SAMPLEPACKET on a port: DCDP to the CPU sysport, collector `SFLOW` (MACs 00:02:04:06:08:0a /
  00:01:03:05:07:09, DSCP 2, TTL 100, SESSION_ID 111), set; sysport `SFLOW_INGRESS` = {ENABLE, set,
  container 1, rate N}; egress: node `EGRESS_SFLOW_ENABLE` is 0 and the sysport set fails NOTFOUND.
- Genetlink hostif (psample): `ifcs_netlink_create` + `netlink_trapid_add` (0.16.2 only; 0.12.41
  has no netlink API).

## SAI ACL tables (`acl.json`: aclorch's L3 / L3V6 tables, groups, entries, ranges, egress)

[acl-20261116-045333: isai `9aecc063`]
- **One node-scope IFCS table per SAI table**: match profile (the table's fields, `PORT_SHARING`=1;
  ranges add `SRC/DST_L4_PORT_COMPARATOR`), action profile (drop, copy, copy override, counter),
  table (`SIZE` 128, `DEFAULT_STATS` off, `MAX_COUNTERS` 128, `SCOPE` node, `PRIORITY` 12-15:
  the CoPP table is 11), one ACL in it. Our 0.12.41 also needs the profile's `MAX_WIDTH`
  (mandatory there, not in 0.16).
- **Binding = `ifcs_acl_port_add(acl, [sysport])`** when the table joins a port's group (egress the
  same with an egress table); unbinding `acl_port_remove`. `IN_PORTS` on part of the bound ports:
  one ACE per port with an `INGRESS_L2_INTERFACE` match (object = sysport).
- Fields: IPs as `SOURCE/DESTINATION_IPV4/6_ADDRESS`, `L4_PROTOCOL` (also for the v6 next header),
  `TCP_FLAGS`, `DSCP` (6 bit), `VLAN_ID`, `ETHER_TYPE`, `ACL_IP_TYPE` as `L3_TYPE` (IPv4 4, +1
  with options; IPv6 6, +1 with extension headers; IPV4ANY = 4/6), ARP via ether type +
  `ARP_OPERATION`; ICMP type/code in `SOURCE_L4_PORT` (v4 tables, protocol 1 added), ICMPv6 type in
  `L4_BYTE1` (v6 tables). Values network order with a MASK.
- Actions: DROP {1,1,1, the ingress-ACL drop reason}; FORWARD = a DROP action with value 0; TRAP =
  COPY_TO_CPU (a user hostif trap, queue 0) + DROP; COPY = COPY_TO_CPU; a counter (`MODE` colour
  blind) per ACE. ACE `PRIORITY` = 0xffffffff - 2 x SAI priority; node max priority 0xffffffff.
- Ranges: `acl_comparator` (`OPERATION` RANGE, `VALUE`, `HI_VALUE`) in a comparator group, matched
  as `COMPARATOR`. A table with REDIRECT / SET_TC / SET_DSCP actions: **RESOURCE_FULL in isai too**.
- Counter read: `acl_counter_stats_get` (packets, bytes, per colour); clear: `acl_counter_stats_clear`.

## Counters, IGMP / MLD / NEIGHBOR_MISS, CPU queues (`hostif-counters.json`)

[hostif-counters-20261116-045615: isai `9e1d7607`]
- Port counters: isai's SAI-id -> devport-counter mapping (`IF_IN_OCTETS` RX_BYTES_ALL, `IF_OUT_OCTETS`
  TX_BYTES_OK, ETHER_STATS_* = rx + tx, ETHER_IN/OUT 64 B with the runts, FEC = the KR/KP FEC codeword
  counters, PFC frames / pause time per priority, WRED = AQM drops) is now ours (`tl_counters.cpp`);
  isai takes discards from sysport drop reasons and its drop counters, IP_*/IPV6_* from sysport
  stats (not ours yet). About 60 ids isai answers with a constant 0.
- CPU queues: `ifcs_cpu_queue_stats_get(queue)` (packets / bytes sent to the CPU, drops).
- IGMP / MLD traps: ether type + protocol + message type in `L4_BYTE1` (no mask). NEIGHBOR_MISS: a
  user hostif trap of its own (the glean routes use it) + an ACE on it. SAMPLEPACKET traps and
  GENETLINK host interfaces: refused by isai too.

## LAG, L2 (`lag-l2.json`, no traffic)

[lag-l2-20261116-043720: isai `1126aa5a`; ours lag-l2-20261116-050410 `78de658d`]
- LAG: `lag_create(0, &h, 0, NULL)`, `lag_member_add(lag, 1, [sysport], 0, NULL)`; a member is removed
  from the IFCS LAG only while egress and ingress are both disabled (learning off then); EGRESS_DISABLE
  alone makes no IFCS call.
- isai gives each VLAN member its own mc_dest when it joins (none per port at init) and keeps the CPU
  out of every VLAN; a LAG joins as its handle (L2VNI member, mc_dest LOCAL_DESTINATION). isai's
  `mdg_mc_dest_add` of the LAG mc_dest fails (INVAL): libifcs wants node
  `MULTICAST_LAG_RESOLUTION_ENABLE`, settable only before node enable; ours sets it, then the LAG joins.
- FDB entries, next hops and RIF transport on a LAG take the LAG handle; flush = `l2_entry_delete_all`
  with dest/type filters (ours walks and deletes). `stp_port_add` refuses a LAG handle (ours).
- Hash: one `extended_field_list` per packet type on node `ENTROPY_SELECT_*` + `ENTROPY_SEED`, shared
  by LAG and ECMP (the last set wins).
- Chip results with traffic and teamd: [sai-hw-l2lag-20261116.txt](sai-hw-l2lag-20261116.txt).

## Running it

`run-oracle.sh SCRIPT both` takes ~3-4 min (isai's create_switch ~20 s, remove ~13 s, tracing
on top); ipd goes to `legacy_abi=0` for the isai run and back to 1 (box-run.sh, also on failure).
