# sai-teralynx

`libsai.so` for the Teralynx 7 (Innovium/Marvell, PCI 1d98:1b58) of the
Celestica CS8210-32X: the [sai-openbcm](../sai-openbcm/README.md) SAI frontend,
**unmodified** (its `src/` is passed in as `FRONTEND`), plus our IFCS backend
in `src/` behind the frontend's backend interface (`backend.h`, the
"openbcm" slot: `SAI_OPENBCM_BACKEND=openbcm`). The vendor SDK (IFCS 0.12.41,
2020, arm64) is **dlopen'ed at run time** from the switch; nothing vendor is
linked, built or stored. Status: passes its tests on x86 against a mock
libifcs; **on the chip (2026-11-16, no syncd yet)**: init, 32 ports, admin/oper
state and link events on the 400G loop, adopted netdevs, temperature, clean
teardown ([evidence](../../../devices/sonic-nixos/cs8210-sai-hw-20261116.txt)), and
host traps with CoPP policing (sai-script `traps-hw.json`, see
[sai-features.md](../../../devices/cs8210-32x/sai-features.md), which tracks what is
done and tested where). The rest below is unverified there.

**libsai-teralynx-shim.so** (`src/shim.c`, next to libsai, dlopen'ed RTLD_GLOBAL
before libifcs): stubs for the 19 CPython 2.7 functions libifcs references
(else `dlopen` fails), and an aligned copy loop that `ifcs_lib.cpp` writes into
libifcs' `memcpy` GOT entries: libifcs copies to/from the PCI BAR, where current
glibc's aarch64 memcpy faults (SIGBUS); the vendor's glibc 2.25 did not. Also
the **learn-ring read guard** (`src/ifcs_guard.c`): libifcs' own
`node_pci_reg_read32` / `_write32` / `im_malloc` GOT entries point at it, so an
all-ones (or out-of-range) read of the learn ring's index registers is re-read
(3 ms apart) and, if it stays bad, turned into "nothing new" instead of a 32 GiB
im_malloc and an endless loop (no re-reads while it stays bad); all-ones from
any other register is counted; im_malloc over 1 GiB returns NULL. Logged to
stderr and counted in `<run dir>/ifcs-guard.prom`
([learn-wedge.md](../../../devices/cs8210-32x/learn-wedge.md)).

## Source layout

One file per area, so areas can be worked on in parallel; `ifcs_backend.cpp` only
owns them and maps the frontend's `Backend` interface onto them.

| file | area |
|---|---|
| `src/shim.c`, `src/ifcs_guard.c` | libsai-teralynx-shim.so: CPython stubs, BAR-safe memcpy, the learn-ring read guard (C, no C++ runtime) |
| `src/tl_core.{h,cpp}` | shared: the libifcs table, lock, port map, sysport handles, status mapping, attribute builders, helpers; pointers to every area |
| `src/tl_switch.*` | init / shutdown, init commands, event registration, temperatures, table usage, chip limits, diag shell |
| `src/tl_port.*` | admin, link (+ optional cache), speed, FEC, autoneg, loopback, MTU, PVID, learning, counters |
| `src/tl_l2.*` | VLANs (L2VNI, STP, flood groups, LAG members as their LAG), FDB, flush, aging, L2 events |
| `src/tl_l3.*` | VRFs, interfaces, my-station, next hops, hosts, routes, ECMP |
| `src/tl_hostif.*` | netdevs, host traps (`trap_program`, the trap → CoPP rule table; IGMP / MLD / NEIGHBOR_MISS), system traps as isai sets them, CPU queue counters |
| `src/tl_acl.*` | policers, CoPP ACL table / entries / meters / counters; SAI ACL tables (fp groups / entries: one node-scope IFCS table each, port binding, counters, meters) |
| `src/tl_counters.*` | port counters by SAI id (isai's mapping, 101 ids) and the diag shell's `counters <devport>` |
| `src/tl_lag.*` | LAGs and members, LAG dests for L2/L3, the switch hash (entropy) |
| `src/tl_qos.*` | QoS maps (qos_map + shared map groups on sysports), PFC, queue schedulers / port shaper, WRED (AQM curve groups), queue buffer thresholds, queue / PG / PFC counters |
| `src/tl_mirror.*` | mirror sessions (DCDP + collector + collector set), port mirroring and sFlow sampling (sysport traffic-monitor policies); `collector_set()` for ACL mirror actions |
| `src/tl_sflow.*` | genetlink host interfaces: sFlow samples to psample through the ipd driver's sFlow netlink (ioctls on libifcs' `/dev/ipd` fd), the ports' sample rates for the samples' metadata; the SAMPLEPACKET trap (MIRROR_TO_CPU, its group's policer as the samples' CPU-queue meter); egress sampling's node attribute |
| `src/ifcs_api.h.in`, `src/ifcs_api_qos.h.in`, `src/ifcs_overrides.txt` | templates of the IFCS declarations (names only; `_qos`: the QoS / mirror areas, `IFCS_SYMBOLS_QOS`) and the 0.12.41 overrides, filled in at build time ([IFCS declarations](#ifcs-declarations)) |
| `tools/gen_ifcs_api.py`, `tools/ifcs_ctypes_model.py` | the generator (into `build/gen/`) and its ctypes reader (parses, never executes) |
| `tests/test_ifcs.cpp`, `tests/t_<area>.cpp`, `tests/harness.h` | end-to-end test: main + one file per area |
| `tests/mock/mock_<area>.c`, `mock.h` | mock libifcs, one file per area; type-checked against the generated `ifcs_api.h`, none missing (link check) |
| `oracle/` | libifcs call tracer + SAI script runner + diff ([oracle/README.md](oracle/README.md)) |
| `tools/` | `sai_smoke.cpp`, `trap_frames.c` (frame sender/counter for trap tests), `qos_frames.c` (DSCP / ECN / PFC frames, per-DSCP, GRE-copy and rate counts for the QoS / mirror / sFlow tests), `raw_frames.c` (any hex frame, tagged and counted: IGMP / MLD / ICMP / DSCP / IPv6 for the ACL and trap tests), `sflow_probe.c` (sequence-numbered frames, samples read from a netdev or psample: 1-in-N, gaps, metadata), `sflow-sonic-test.sh` (SONiC sFlow by hand on the box: patched ipd, libsai drop-in, sflowmgrd / hsflowd / sflowtool) |

The Makefile picks up `src/*.cpp`, `tests/t_*.cpp` and `tests/mock/*.c` by wildcard.

## Profile keys

| key | meaning |
|---|---|
| `SAI_OPENBCM_BACKEND` | `openbcm` = this backend (`fake` = the frontend's in-memory model) |
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
| `SAI_TERALYNX_IFCS_TRACE` | path of the oracle's `libifcs-trace.so` (ours): every IFCS call traced ([oracle/README.md](oracle/README.md)) |
| `SAI_OPENBCM_TEMP_FILE`, `_TEMP_PROM`, `_DIAG_SOCKET` | frontend meaning (fand temperature file, Prometheus file, diag socket) |

The frontend collects every key through the service table's
`profile_get_next_value` (syncd has it); the backend also asks
`profile_get_value` directly for its own keys.

## Run-time vendor files (on the switch, never in the store)

| file (SDK dir) | sha256 | use |
|---|---|---|
| `libifcs.so` | `388031e7eb2443d77d89da826084976859bf92db19f9733be19b1c0b603c0d17` | dlopen'ed (`RTLD_NOW|RTLD_GLOBAL`), the `IFCS_SYMBOLS` list ([src/ifcs_api.h.in](src/ifcs_api.h.in)); a missing one fails init naming it |
| `libAAPL.so` | `cf4e848498398d4dca083156d9819bfda4c006eebf92c66729a2cdd40d2c41a6` | libifcs dependency, plus jansson/libyaml/zlib, via `LD_LIBRARY_PATH` |
| `cel_configs/isg.yaml` / `isg_R01.yaml` (board id 0x2) | `377e3b97…` / `c1f85dd7…` | `SAI_TERALYNX_CONFIG_APPEND` |
| `cel_cmds/*.cmd` | e.g. `misc_config_32.cmd` `c8847694…` | `SAI_TERALYNX_INIT_CMDS` |

Build-time vendor input (store only, never git): Innovium's public
`ishell.deb` 2.0.1 (`Innovium/SONiC` `debian/202111/T2.0.1` at `240ee432`,
sha256 `894ca773803d0149baf14732a13603740920bce5f015b3437e3dbc4761f7f5e9`), of which
only `innovium/pyctypes/ifcs_ctypes.py` (sha256 `5a4547fb…`) is used
(`default.nix` `ifcsCtypes`).

## IFCS declarations

`build/gen/ifcs_api.h` and `ifcs_api_qos.h` are generated at every build and
never committed. `tools/gen_ifcs_api.py` fills the templates `src/ifcs_api*.h.in`
(ours: which names, their grouping, our macros and comments, and the 0.12.41
layout static_asserts) with the values, field lists and signatures of a ctypes
file, read as data by `tools/ifcs_ctypes_model.py`. Directives and rules: the
generator's docstring. Outside Nix: `make IFCS_CTYPES=<ifcs_ctypes.py>`.

The input is the public ishell 2.0.1 ctypes above, not the box's 0.12.41 one.
Checked 2026-11-17 ([evidence](../../../devices/cs8210-32x/ifcs-ctypes-public-20261117.txt)):

- From the box's own 0.12.41 `pyctypes/ifcs_ctypes.py` (sha256 `ba980c0b…`, local
  only) without overrides, the generator writes the same headers, byte for byte,
  as from ishell 2.0.1 with `src/ifcs_overrides.txt`.
- Every public ishell from 2.0.1 to 3.1.3 differs from 0.12.41 in the same three
  places for what we use; 3.1.4 and later in more. No public artifact is 0.12.41
  itself (the isai debs before 2.0.1 ship no ctypes, headers or DWARF).
- The three: `IFCS_DEVPORT_STATS_ID_MAX_COUNT` / `_ALL` (2.0.1 has three more
  counters; 0.12.41's 111 / 112 are overrides, verified in the target library's
  `ii_devport_stats_get`), and `ifcs_usage_t` (0.12.41 has only `max`, `current`:
  `@prefix`; its `ifcs_usage_t_init` clears 12 bytes). The unused
  `IFCS_DEVPORT_ATTR_MAX_COUNT` was dropped instead.
- The outputs built from the generated headers equal the ones from the former
  hand-written `src/ifcs_api*.h` (native and aarch64 sai-teralynx, the aarch64
  oracle: every file identical once the outputs' own store paths are masked;
  test_ifcs 1015/0, frontend 2667/0). The DWARF of both headers: 669 typedefs
  and enumerators identical, only the dropped one missing.

The static_asserts (sizes and offsets from the DWARF of the SDK's
`innovium.user`, or computed from its pyctypes; marked) stop a build whose
input disagrees with 0.12.41. Local layout evidence (git-ignored):
`devices/cs8210-32x/analysis/sai-teralynx/` (`innovium-user-main.txt`,
`layouts-innovium-user-dwarf.txt`, `vendor-cmds-parse.txt`, `pahole-libifcs-7.0.1.txt`).

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
| counters, clear | every SAI port counter isai maps (101 ids, `tl_counters.cpp`): one `ifcs_devport_stats_get` of the IFCS ids the request needs (`port_stats_by_id`), `_clear`; IF_OUT_OCTETS = TX_BYTES_OK (isai) | V (on chip) |
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
| L3 interface | `ifcs_intf_create` SVI on the VLAN's L2VNI (port RIFs: the frontend's internal VLAN) | I (sample uses L3PORT) |
| my-station | no IFCS object (an intf terminates its own MAC) | I |
| next hop | `ifcs_nexthop_create` intf + dest MAC + sysport; update in place | V / I |
| routes, hosts | `ifcs_route_entry_create` IP_DEST_L3VNI key; NEXTHOP + FWD forward | V |
| DROP route | `FWD_POLICY` drop, no NEXTHOP | I |
| TRAP route (ip2me, glean) | `FWD_POLICY` drop + `CTC_POLICY` copy to the IP2ME hostif trap (queue 0 until the IP2ME SAI trap sets one) | V (on chip: ip2me frames reached the netdev) |
| ECMP | `ifcs_ecmp_create` + `member_add`, repeats = `WEIGHT` | I |
| temperatures | node `MASTER_LAST_TEMP` + `REMOTE0..3` (float C), one monitor per read (~400 ms, holds the libifcs lock), round-robin, at most one read per 15 s | V (scripts/temp.py) |
| chip limits | node ACL priority range, `NUMBER_OF_ECMP_GROUPS`; 64 paths/group (ours); buffer size 0 (unknown) | V / - |
| table usage | prefixes/hosts from our count vs [fib-capacity.md](../../../devices/cs8210-32x/fib-capacity.md) (24,576 TCAM, no ILPM); nexthop/L2/ECMP `*_usage_get` | I |
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

## Tests

`make check` (native, also the nix build's checkPhase):
- `tests/test_ifcs.cpp` + `tests/t_<area>.cpp` drive this libsai through the
  public SAI API with the mock libifcs (`tests/mock/`, records calls), one
  switch, areas in order: refusals before anything is loaded; the exact init
  call sequence; merged config; init commands; ports, FEC, MTU, link events,
  link cache, counters by id list; netdevs; VLANs (per-member mc_dests, no
  CPU), FDB, events, flush by port / VLAN / all, aging; routes, RIF, neighbor,
  next hop, ip2me; LAGs (members, egress/ingress disable, LAG bridge port in
  VLANs, PVID, FDB on the LAG, member join/leave in a VLAN, port RIF), the hash
  profile; host traps and CoPP (table,
  6 IBs, entries, meters, policer update, DROP, TTL/MTU system traps, BGP on
  IP2ME, unmapped type kept, group queue change, removal); QoS (maps + shared
  map groups, TC -> queue identity rule, PFC, schedulers via scheduler groups,
  port / queue shapers in bits/s, WRED curves, queue buffer reserve with the
  port flapped, queue / PG / PFC counters); mirror (SPAN / ERSPAN collectors,
  shared DCDP, session update inside the set, sFlow collector + policy); sFlow to psample (`t_sflow.cpp`: GENETLINK hostif,
  SAMPLEPACKET table entry -> ipd ioctls in order, rates on sampler changes, refusals, failing ioctls);
  SAI ACL tables (L3 / L3V6 as aclorch, group binding, every field and
  action the frontend passes, IN_PORTS on part of the ports, policer, refusals,
  removal); counters by SAI id, CPU queues and the diag shell (`acl`, `counters`);
  the optional hooks (`t_hooks.cpp`: RIF counters / admin state, glean vs IP2ME traps,
  port counters by id + stats capability, hash objects, VLAN flood control, storm
  control, ACL ranges / SET_TC / REDIRECT / MIRROR / egress + OUT_PORTS, entry admin
  and in-place update, AVAILABLE_ACL_ENTRY, SET_DSCP refused); temperatures
  (round-robin, cached); remove_switch order; the SAMPLEPACKET trap's queue meter and egress sampling (`t_sflow.cpp`);
  the shim's learn-ring guard (`t_learn.cpp`: the mock replays libifcs' learn pass through its PLT with injected
  all-ones reads; the GOT redirect uses the build host's relocation types, so x86 runs it). 1047 checks (all areas, 2026-11-17).
- sai-openbcm's `tests/test_sai.cpp`, unchanged, against this library's
  objects (frontend + fake backend): `test_sai` 3012 + `test_units` 140 checks (2026-11-23).
- Built natively with nix (`callPackage ./sai-teralynx { inherit sonic; }`) and
  for aarch64 (static_asserts hold). `TEST_IFCS_DUMP=1` prints the IFCS calls.
- On the chip: `oracle/scripts/traps-hw.json` (frames over the loop,
  `tools/trap_frames.c`), `qos-hw.json` (`tools/qos_frames.c`: classification,
  shapers, DWRR / strict, mirror copies, sFlow), `acl-hw.json` / `acl-hw2.json` (ACL actions and
  matches on routed and ip2me frames, IGMP / MLD traps, CPU queues, counters; `tools/raw_frames.c`),
  `hostif-create.json`, `perf.json`, `l3-*.json` (with a loop-observer ACL: routed copies to the far
  hosts' MACs are trapped, `tools/l3_frames.c`), `phasec-hw.json` (the hooks above with traffic),
  `phasec-recreate.json` (two switches in one process), `sflow-rate.json` / `sflow-rate2.json` / `sflow-genl.json`
  (`tools/sflow_probe.c`: sampling statistics, psample delivery), sai_bench (qemu-ipd); results in
  [sai-features.md](../../../devices/cs8210-32x/sai-features.md).

## Only the chip can tell

Every I row above, and: which init yaml makes `ifcs_netdev_create` produce a
netdev (with the deployed one it returns SUCCESS and none appears) and whether
carrier follows link; LAG hash distribution and multi-member LAGs (one cable:
single-member LAGs only); the minimum meter CIR; whether ACL tables of different
priorities resolve conflicting actions by priority (all hit in parallel: the CoPP
and a user ACE both counted the same frames); the unknown-unicast storm entry's
DESTINATION_MAC_HIT semantics (installed, not exercised); VLAN flood control's
effect (one loop: no second VLAN member); queue RESERVED_SIZE (refused by 0.12.41).
