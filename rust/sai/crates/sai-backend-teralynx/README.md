# sai-backend-teralynx

The Teralynx 7 backend of the Rust SAI for the Celestica CS8210-32X (Innovium/Marvell, PCI 1d98:1b58):
`libsai.so` = `sai-ffi` with feature `teralynx`, the vendor SDK (IFCS 0.12.41, 2020, arm64) `dlopen`ed at run
time from the switch through the `ifcs` rim; nothing vendor is linked, built or stored. Design, unsafe rules,
the shim (`ifcs-shim`, the learn-ring guard) and the parity harness: [rust/sai/README.md](../../README.md)
"Teralynx backend", "Teralynx shim", "Parity". Hardware status: [sai-features.md](../../../../ai_re/devices/cs8210-32x/sai-features.md).

This is a port, area by area with the same IFCS calls, of the C++ IFCS backend of the `sai-teralynx` package
(deleted in H5 on 2026-11-27, last in the tree at commit 767d9b0). The sections below are that package's
README, moved here (its profile keys, vendor files, init sequence and mapping are this crate's).

## Profile keys

| key | meaning |
|---|---|
| `SAI_OPENBCM_BACKEND` | `openbcm` = this backend (`fake` = the core's in-memory model) |
| `SAI_INIT_CONFIG_FILE` | generated IFCS init yaml (all but the isg lane-swap section); required |
| `SAI_TERALYNX_CONFIG_APPEND` | optional, appended at run time (the vendor isg yaml in the SDK root) |
| `SAI_TERALYNX_RUN_DIR` | where the merged `config.yaml` is written (default `/run/sai-teralynx`) |
| `SAI_TERALYNX_LIBIFCS` | absolute path of the vendor `libifcs.so`; required |
| `SAI_TERALYNX_PORT_MAP` | `devport lanes speed_mbps fec` per front-panel devport (lanes `,`-separated, `serdes_group*8+lane+1`; fec `none|rs|fc`; `#` comments); required |
| `SAI_TERALYNX_INIT_CMDS` | `:`-separated command files applied after node enable (below) |
| `SAI_TERALYNX_NETDEV` | `adopt` (default): rename the SDK's `inno_0_<sysport>` (init yaml with netdev `auto_create`, the vendor `-k` mode); `create`: `ifcs_netdev_create` per hostif, which makes no netdev on the deployed init yaml (chip 2026-11-16) |
| `SAI_TERALYNX_L3INTF_STATS`, `_MODES`, `_COUNTERS` | RIF counters: `off` skips the node setup; modes tried before node enable (default `4,1,2,3`: 0.12.41 refuses 1 = ALL, takes 4 = IPv4 + IPv6), flex counters per direction (default 4096) |
| `SAI_TERALYNX_SYSTEM_TRAPS` | `isai` (default): system traps as isai sets them at init (TTL, IPv4 options on; L2 source miss/move, L3 MTU off), before the first netdev or trap; `sdk`: leave the SDK's |
| `SAI_TERALYNX_SFLOW_CPU_QUEUE` | CPU queue the init yaml's `traffic_monitor_cpu_queue` puts sFlow samples on, when node `TRAFFIC_MONITOR_CPU_QUEUE` is unreadable; with `SAI_TERALYNX_SFLOW_METER=on` the SAMPLEPACKET trap group's policer meters it if it is the group's queue |
| `SAI_TERALYNX_SFLOW_METER` | `on`: the sflow trap group's policer as the samples' CPU-queue meter; default `off` (on the chip the meter did not hold its rate and once stopped all samples, sai-features.md) |
| `SAI_TERALYNX_EGRESS_SFLOW` | `on`: node `EGRESS_SFLOW_ENABLE` before node enable (egress sampling; the yaml's `egress_sflow_enable` is ignored by 0.12.41); default `off` |
| `SAI_TERALYNX_SHUTDOWN` | `deinit` (default): ports down, then `ifcs_deinit(COLD)`; `ports-down`: ports down only |
| `SAI_TERALYNX_CPU_FLOOD` | `off` (default): the CPU is in no VLAN (traps bring what it needs); `on`: CPU in every VLAN's L2VNI and flood group (the old model: COPY traps doubled) |
| `SAI_TERALYNX_LAG_FLOOD` | `on` (default): node `MULTICAST_LAG_RESOLUTION_ENABLE` before node enable (a LAG in a VLAN needs it); `off`: skip (LAGs cannot join VLANs) |
| `SAI_TERALYNX_HASH_FIELDS`, `_HASH_SEED` | LAG/ECMP hash at init (the SAI hash objects set it at run time too): SAI native field names in lower case (`src_ip,dst_ip,ip_protocol,l4_src_port,l4_dst_port,in_port`, also `src_mac dst_mac ethertype vlan_id ipv6_flow_label`); node `ENTROPY_SEED`. Unset: the SDK's selection |
| `SAI_TERALYNX_IFCS_LOG_NAME` | 2nd argument of `ifcs_init` (the SDK's log name, `$II_ROOT/log/<name>.log`); default `sai-teralynx` |
| `SAI_TERALYNX_LINK_STATUS` | `read` (default): one `LINK_STATUS` read per OPER_STATUS get (~5 us); `cached`: from link events, no IFCS call (a transition the link scan misses would stay wrong) |
| `SAI_TERALYNX_STATS` | `ids` (default): one `devport_stats_get` of the 12 mapped counters per port; `all`: the vendor CLI form (all 112, 8x slower) |
| `SAI_TERALYNX_TEMP_PERIOD` | seconds between temperature reads (default 15, `0` = every call); each read takes one monitor, round-robin, because a monitor read holds the libifcs lock ~400 ms |
| `SAI_TERALYNX_TEMP_MONITORS` | `all` (default: master + 4 remotes) or `master` |
| `SAI_TERALYNX_IFCS_GUARD` | `on` (default): the learn-ring read guard (shim); `off`: libifcs' register reads and im_malloc untouched |
| `SAI_TERALYNX_LEARN_RING_SIZE`, `SAI_TERALYNX_IM_MALLOC_LIMIT_MIB` | the guard's ring size (default 2048, the deployed init yaml) and im_malloc limit (default 1024; 0 = none) |
| env `SAI_TERALYNX_IFCS_GUARD_INJECT` | test hook only: a file "<reg> <count>" there (checked once a second, then removed) makes the next reads of that register return all-ones |
| `SAI_TERALYNX_IFCS_TRACE` | path of the oracle's `libifcs-trace.so` (ours): every IFCS call traced ([ifcs-oracle](../../../../ai_re/nix/packages/ifcs-oracle/README.md)) |
| `SAI_OPENBCM_TEMP_FILE`, `_TEMP_PROM`, `_DIAG_SOCKET` | the core's meaning (fand temperature file, Prometheus file, diag socket) |

The rim collects every key through the service table's `profile_get_next_value` (syncd has it); the
backend also asks `profile_get_value` directly for its own keys (`src/keys.rs`).

## Run-time vendor files (on the switch, never in the store)

| file (SDK dir) | sha256 | use |
|---|---|---|
| `libifcs.so` | `388031e7eb2443d77d89da826084976859bf92db19f9733be19b1c0b603c0d17` | dlopen'ed (`RTLD_NOW|RTLD_GLOBAL`), the `IFCS_SYMBOLS` list ([ifcs-gen templates/ifcs_api.h.in](../ifcs-gen/templates/ifcs_api.h.in)); a missing one fails init naming it |
| `libAAPL.so` | `cf4e848498398d4dca083156d9819bfda4c006eebf92c66729a2cdd40d2c41a6` | libifcs dependency, plus jansson/libyaml/zlib, via `LD_LIBRARY_PATH` |
| `cel_configs/isg.yaml` / `isg_R01.yaml` (board id 0x2) | `377e3b97…` / `c1f85dd7…` | `SAI_TERALYNX_CONFIG_APPEND` |
| `cel_cmds/*.cmd` | e.g. `misc_config_32.cmd` `c8847694…` | `SAI_TERALYNX_INIT_CMDS` |

Build-time vendor input (store only, never git): Innovium's public
`ishell.deb` 2.0.1 (`Innovium/SONiC` `debian/202111/T2.0.1` at `240ee432`,
sha256 `894ca773803d0149baf14732a13603740920bce5f015b3437e3dbc4761f7f5e9`), of which
only `innovium/pyctypes/ifcs_ctypes.py` (sha256 `5a4547fb…`) is used
(`rust/sai/nix/ifcs-ctypes.nix`).

## IFCS declarations

`ifcs_api.h` and `ifcs_api_qos.h` are generated at every build and never committed: crate `ifcs-gen` fills
the templates `crates/ifcs-gen/templates/ifcs_api*.h.in` (ours: which names, their grouping, our macros and
comments, the 0.12.41 layout static_asserts) and the overrides `ifcs_overrides.txt` with the values, field
lists and signatures of a ctypes file read as data (`ifcs-sys`'s build; `nix/ifcs-headers.nix` for the C
fixtures, checked against `crates/ifcs-gen/tests/golden/ishell-2.0.1.sha256`). The Python generator it ports
(the C++ package's `tools/gen_ifcs_api.py`, deleted in H5 on 2026-11-27, last in the tree at commit 767d9b0)
wrote the same bytes and read the same model (43,268 facts), proven live until then.

The input is the public ishell 2.0.1 ctypes above, not the box's 0.12.41 one. Checked 2026-11-17
([evidence](../../../../ai_re/devices/cs8210-32x/ifcs-ctypes-public-20261117.txt)):

- From the box's own 0.12.41 `pyctypes/ifcs_ctypes.py` (sha256 `ba980c0b…`, local only) without overrides,
  the generator writes the same headers, byte for byte, as from ishell 2.0.1 with the overrides.
- Every public ishell from 2.0.1 to 3.1.3 differs from 0.12.41 in the same three places for what we use;
  3.1.4 and later in more. No public artifact is 0.12.41 itself.
- The three: `IFCS_DEVPORT_STATS_ID_MAX_COUNT` / `_ALL` (0.12.41's 111 / 112 are overrides, verified in the
  target library's `ii_devport_stats_get`), and `ifcs_usage_t` (0.12.41 has only `max`, `current`:
  `@prefix`; its `ifcs_usage_t_init` clears 12 bytes).

The static_asserts (sizes and offsets from the DWARF of the SDK's `innovium.user`, or computed from its
pyctypes; marked) stop a build whose input disagrees with 0.12.41. Local layout evidence (git-ignored):
`ai_re/devices/cs8210-32x/analysis/sai-teralynx/`.

## Init and shutdown

Exactly the vendor's `innovium.user` (disassembly): `ifcs_init(config,
<log name>, IFCS_BOOT_COLD, &num_nodes)`, `ifcs_node_create(0, 0, NULL)`, one
attr via `ifcs_attr_t_init` / `_id_set(0 = NODE_ATTR_ENABLE)` /
`_value_data_set(1)`, `ifcs_node_attr_set(0, 1, &attr)`. Then: every port-map
devport admin-down, the init commands, log / link scan / L2 event callbacks,
one STP instance (all ports FORWARDING), VLAN 1 (empty, no CPU), the hash
profile. One addition between node create and enable: node
`MULTICAST_LAG_RESOLUTION_ENABLE` (libifcs refuses it once the node is
enabled, and refuses a LAG mc_dest in a flood group without it). Shutdown: all ports admin-down first, callbacks off, netdevs deleted,
`ifcs_deinit(COLD)` (the vendor's own stop is `kill -9`).

Init commands: only `ifcs set devport <id> <attr> <value|[list]>` for the
attributes `SDK/cel_cmds/{misc_config_32,dynamic_misc,dynamic_preemphasis,preemphasis_*}.cmd`
use (`rx_eq_fine_tune`, `link_training`, `tx_eq_{pre1,pre2,pre3,attn,post}`,
`rx_ctle_{lf,hf,bw}`, `rx_gainshape{1,2}`, `rx_agc_target`,
`rx_eye_disqualify_threshold_25g`), typed as the vendor CLI's setters do. All
vendor files parse. Everything else is skipped with one warning per kind:
**`port enable`** (ports stay down until SAI enables them), `node create`,
`shell echo`, and **`pci write led_*` (LED stream setup: not done by this libsai)**.

## What is mapped

V = call form seen in vendor code (CLI python, sample scripts, innovium.user);
I = inferred (API exists, form or semantics not seen in use).

| backend call | IFCS | |
|---|---|---|
| ports / cpu_port | port map; CPU = devport 0 (sysport 1000) | V |
| admin, link | devport `ADMIN_STATE`, `LINK_STATUS` (read per get; `cached` mode: from link events) | V |
| link events | `ifcs_linkscan_event_register_notify` -> queue | I (struct from pyctypes) |
| speed get / set | `SPEED`; set = configured speed only, else NOT_SUPPORTED | V / - |
| FEC | `FEC_MODE`: RS keeps the yaml's KR/KP, else KP for >= 200G or PAM4 lanes, KR otherwise; FC | V attr, I mapping |
| autoneg, loopback | `AUTO_NEG`; MAC -> PCS, PHY -> PMA loopback | V attr, I mapping |
| frame max (MTU) | sysport `MTU` (L2 MTU) = SAI MTU + 22, max 9290 | I |
| PVID, learning | sysport `DEFAULT_CVID`, `LEARN_ENABLE` (drop/cpu modes NOT_IMPLEMENTED) | V |
| counters, clear | every SAI port counter isai maps (101 ids, `src/counters.rs`): one `ifcs_devport_stats_get` of the IFCS ids the request needs (`port_stats_by_id`), `_clear`; IF_OUT_OCTETS = TX_BYTES_OK (isai) | V (on chip) |
| RIF counters, admin state | intf stats (node L3INTF stats CONFIG + MODE before enable; IN = ingress v4 + v6), intf IPV4/IPV6_UNICAST_ENABLE | V (on chip) |
| glean, IPv4 options | glean routes copy to a hostif trap of their own (NEIGHBOR_MISS's queue); L3VNI IPV4_OPTIONS_CTC + _DROP | V (on chip) |
| storm control, VLAN flood control | an ingress ACL table of its own (entry per port and kind, policer meter, colour counters); L2VNI FLOOD_GROUP = an empty MDG when all three kinds are off | V (chip: broadcast storm, flood accepted) |
| ACL extras | comparators (L4 port ranges), SET_TC, REDIRECT (nexthop / sysport / LAG), MIRROR (collector set), egress tables (no port match, bound per port), ACE ENABLE, in-place entry update (counter kept); SET_DSCP refused by 0.12.41 | V (on chip) |
| netdev | `ifcs_netdev_create` (name, devport, MAC, VLAN tag) or adopt; keep_tag = `VLAN_TAG` | I / V (adopt = knet.py) |
| netdev carrier | no IFCS call: libifcs drives it from link scan | I |
| VLAN | L2VNI(vid) (STP, flood group, `LEARN_ENABLE`, max learn count as isai) + MDG flood group with one mc_dest per member, created when it joins (isai); the CPU in no VLAN; a LAG member joins as its LAG (one L2VNI member + mc_dest per LAG, counted); untagged = sysport `EGRESS_UNTAGGED_VLAN_ID` (one per port, set on each member) | V (oracle, chip) |
| FDB add / delete | `ifcs_l2_entry_create` MAC_L2VNI key, `ENTRY_DEST` (sysport or LAG handle), `ENTRY_TYPE`; discard = `FWD_POLICY` drop | V (oracle, chip) |
| FDB flush | walk `ifcs_l2_entry_get_all`, check type/dest, delete one by one | V calls |
| FDB events | `ifcs_l2_event_register_notify` (learned/moved/aged); dest = sysport or LAG handle (a LAG member's sysport maps to its LAG) | V (chip: learned + aged, port and LAG) |
| aging | node `L2_ENTRY_AGING_INTERVAL`, seconds | V (chip: 10 s ages an entry within 33 s) |
| VRF | `IFCS_HANDLE_L3VNI(vrf+1)`, created on first use; up to 255 | V form |
| L3 interface | `ifcs_intf_create` SVI on the VLAN's L2VNI (port RIFs: the core's internal VLAN) | I (sample uses L3PORT) |
| my-station | no IFCS object (an intf terminates its own MAC) | I |
| next hop | `ifcs_nexthop_create` intf + dest MAC + sysport; update in place | V / I |
| routes, hosts | `ifcs_route_entry_create` IP_DEST_L3VNI key; NEXTHOP + FWD forward | V |
| DROP route | `FWD_POLICY` drop, no NEXTHOP | I |
| TRAP route (ip2me, glean) | `FWD_POLICY` drop + `CTC_POLICY` copy to the IP2ME hostif trap (queue 0 until the IP2ME SAI trap sets one) | V (on chip: ip2me frames reached the netdev) |
| ECMP | `ifcs_ecmp_create` + `member_add`, repeats = `WEIGHT` | I |
| temperatures | node `MASTER_LAST_TEMP` + `REMOTE0..3` (float C), one monitor per read (~400 ms, holds the libifcs lock), round-robin, at most one read per 15 s | V (scripts/temp.py) |
| chip limits | node ACL priority range, `NUMBER_OF_ECMP_GROUPS`; 64 paths/group (ours); buffer size 0 (unknown) | V / - |
| table usage | prefixes/hosts from our count vs [fib-capacity.md](../../../../ai_re/devices/cs8210-32x/fib-capacity.md) (24,576 TCAM, no ILPM); nexthop/L2/ECMP `*_usage_get` | I |
| diag shell | read-only `ports`, `devport <id>`, `temp`, `copp` (traps, queues, CoPP counters, system traps that fired), `help` | |
| LAG | `ifcs_lag_create(0, &h, 0, NULL)`, `lag_member_add/remove(lag, 1, [sysport])`; a member is out of the IFCS LAG while egress and ingress are both disabled (isai's rule), learning off then; libifcs refuses a member in an L2VNI / mc_dest / L2 entry, so ports in no VLAN hold none; STP stays per member sysport (`stp_port_add` refuses a LAG) | V (oracle, chip) |
| hash | per packet type an `ifcs_extended_field_list` (ELEMENTS) on node `ENTROPY_SELECT_*`, node `ENTROPY_SEED`; one entropy for LAG and ECMP | V (oracle; chip: accepted, distribution untested) |
| SAI ACL tables (fp groups / entries) | per table: match profile (fields, `PORT_SHARING`, `MAX_WIDTH` by key bits: mandatory in 0.12.41), action profile (drop, copy, copy override, counter, meter), node-scope table (size 256, `DEFAULT_STATS` off, priority 12-15), one ACL bound to the entries' ports (`acl_port_add`); per entry one ACE (or one per port with `INGRESS_L2_INTERFACE` for IN_PORTS on part of them), values + masks in network order, DROP / zero DROP (FORWARD) / COPY_TO_CPU (shared ACL hostif trap, queue 0) / METER / counter, priority 0xffffffff - 2 x SAI priority; counter reads lag ~1 s (SDK) | V (isai, oracle acl; on chip) |
| host traps (`trap_program`) | per SAI trap: a hostif trap (user trap with the group's `QUEUE_NUM`; IP2ME = the TRAP routes' one; TTL/MTU = system traps 17/23 with `ENABLE`; NEIGHBOR_MISS = a glean trap of its own) and CoPP entries (below; IGMP / MLD: ether type + protocol + L4 byte 1); SAMPLEPACKET: tl_sflow (below); types without a mapping NOT_SUPPORTED (kept) | V (isai's model, oracle) |
| CoPP table | ingress control table, scope PER_IB, one ACL per inno block (6, probed); match DMAC / ether type / L4 proto / ports / L4 byte 1 / hostif trap; actions copy, copy override, drop, meter, counter; table needs `DEFAULT_STATS` off and `MAX_METERS` | V (on chip; isai's `TYPE` attribute refused by 0.12) |
| trap entries | one ACE per rule and IB: L2 protocols by DMAC + ether type, IP protocols by ether type + L4, BGP/SSH/SNMP/BFD = protocol + IP2ME trap, IP2ME = hostif trap match; COPY_TO_CPU (all colours unless policed), COPY_TO_CPU_OVERRIDE for policed colours, DROP for TRAP, METER, colour counter; priority lower wins | V (on chip) / I (priority direction) |
| sFlow to the host (GENETLINK hostif + SAMPLEPACKET table entry) | no IFCS call: ipd ioctls IPD_SET_PARAMS (sample GRE protocol 0x8818), IPD_CREATE_NETLINK (SFLOW, the hostif's name / group), IPD_SET_NETDEV sample rate per sysport; the driver strips the sample frames and sends psample messages (kernel psample with ipd `psample=1`) | V (ipd source; chip: SONiC + hsflowd + sflowtool) |
| SAMPLEPACKET trap, its group | system trap 512 MIRROR_TO_CPU `ENABLE` (isai); group policer = `cpu_queue_attr_set` METER_MODE / CIR (kbit/s, packets x 84 B) / CBS (B) / CIR_ENABLE on the samples' queue (init yaml `traffic_monitor_cpu_queue`), only when it is the group's queue and `SAI_TERALYNX_SFLOW_METER=on` (off by default). isai's ACE on the collector (MIRROR_2): 0.12.41 refuses a MIRROR match | V trap (oracle sflow-police) / I meter (chip: accepted, rate not held; METER_MODE refused) |
| egress sampling | node `EGRESS_SFLOW_ENABLE` before enable (`SAI_TERALYNX_EGRESS_SFLOW`), read back per egress sampler change | I (chip: egress samples, cut to 176 B) |
| policers | IFCS `acl_meter` per policer and IB (srTCM = RFC 2697, trTCM = 2698, colour aware), packets as 84 B (CIR x 672 bit/s, CBS x 84 B, isai's conversion); updated in place | V (on chip: 100 pps policer held a 1000 pps burst to ~95 pps); CIR 10 pps rejected (range check) |

## Only the chip can tell

Every I row above, and: which init yaml makes `ifcs_netdev_create` produce a
netdev (with the deployed one it returns SUCCESS and none appears) and whether
carrier follows link; LAG hash distribution and multi-member LAGs (one cable:
single-member LAGs only); the minimum meter CIR; whether ACL tables of different
priorities resolve conflicting actions by priority (all hit in parallel: the CoPP
and a user ACE both counted the same frames); the unknown-unicast storm entry's
DESTINATION_MAC_HIT semantics (installed, not exercised); VLAN flood control's
effect (one loop: no second VLAN member); queue RESERVED_SIZE (refused by 0.12.41).
