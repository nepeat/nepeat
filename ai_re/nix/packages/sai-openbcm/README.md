# sai-openbcm: a thin SAI on the Broadcom OpenBCM SDK

`libsai.so` implementing the OCP Switch Abstraction Interface (SAI 1.19, the
headers sairedis is pinned to) on top of Broadcom's source-available OpenBCM
SDK 6.5.27, so SONiC's `syncd` can drive the BCM56930 (Tomahawk, driver
BCM56960_A0) of the AS7760-32X from its big-endian ppc64 control plane, where
no vendor SAI binary exists. Status: builds natively (fake) and cross
(openbcm); everything above the SDK is tested on x86_64 with the fake
backend; smoke-tested on the AS7760 (2026-11-15: switch create, ports, VLAN, RIF;
[sonic-nixos PROGRESS](../../../devices/sonic-nixos/PROGRESS.md)). LAG, SAI
temperatures, the syncd temperature publisher and `--shutdown-at` are
fake-tested only.

## Design

```
 syncd (sairedis-openbcm)
   | sai_api_query / generic create,get,set,remove (C ABI, SAI 1.19)
 SAI layer  src/sai_*.cpp      object model, OIDs, attribute handling, events
   | ob::Backend (src/backend.h): ~55 operations, SAI status codes
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
| `SAI_OPENBCM_FAKE_PORTS` / `_LANES` / `_SPEED` | 32 / 4 / 100000 | fake: port count, lanes per port, initial speed |
| `SAI_OPENBCM_FAKE_LINK` | `admin` | fake: link follows admin state; `none`: never (loopback still links) |

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
- No config file, no init. No fake on a host with BDE devices.
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
| SWITCH | create (INIT_SWITCH=true only), remove; G: TYPE, PORT_NUMBER, PORT_LIST, CPU_PORT, MAX_NUMBER_OF_SUPPORTED_PORTS, DEFAULT_VIRTUAL_ROUTER_ID, DEFAULT_VLAN_ID, DEFAULT_1Q_BRIDGE_ID, DEFAULT_STP_INST_ID, DEFAULT_TRAP_GROUP, PORT_MAX_MTU, OPER_STATUS, QOS_MAX_NUMBER_OF_TRAFFIC_CLASSES, NUMBER_OF_(UNICAST_)QUEUES, MAX_NUMBER_OF_TEMP_SENSORS, TEMP_LIST / MAX_TEMP / AVERAGE_TEMP (live PVTMON read, whole C rounded); C/S/G: SRC_MAC_ADDRESS, FDB_AGING_TIME, all *_NOTIFY pointers (port state + FDB events are raised) | ECMP/LAG hash seed/algorithm/offset, RESTART_WARM, WARM_RECOVER, PRE_SHUTDOWN, FAST_API_ENABLE, UNINIT_DATA_PLANE_ON_REMOVAL, VXLAN_DEFAULT_PORT, BCAST/MCAST_CPU_FLOOD_ENABLE, SWITCHING_MODE, FDB_*_MISS_PACKET_ACTION, COUNTER_REFRESH_INTERVAL, SWITCH_ID, SWITCH_PROFILE_ID; HARDWARE_ACCESS_BUS etc. ignored at create; SWITCH_HARDWARE_INFO returns empty | ECMP_HASH/LAG_HASH objects, VXLAN_DEFAULT_ROUTER_MAC, ACL/resource counters, switch stats, INIT_SWITCH=false |
| PORT | G: TYPE, HW_LANE_LIST (SDK physical lanes), OPER_STATUS (live), SPEED/OPER_SPEED (live), SUPPORTED_SPEED, SUPPORTED_FEC_MODE, SUPPORTED_AUTO_NEG_MODE, QOS_NUMBER_OF_QUEUES, QOS_QUEUE_LIST, NUMBER_OF_INGRESS_PRIORITY_GROUPS, INGRESS_PRIORITY_GROUP_LIST, QOS_NUMBER_OF_SCHEDULER_GROUPS (0); S/G: ADMIN_STATE, SPEED, MTU (frame max = MTU + 8), FEC_MODE (NONE/RS/FC), AUTO_NEG_MODE, PORT_VLAN_ID, LOOPBACK_MODE (NONE/MAC/PHY); stats: IF_IN/OUT_{OCTETS,UCAST,NON_UCAST,BROADCAST,MULTICAST,DISCARDS,ERRORS}, IF_IN_UNKNOWN_PROTOS (read; clear-all) | MEDIA_TYPE, INTERFACE_TYPE, DEFAULT_VLAN_PRIORITY, DROP_(UN)TAGGED, LINK_TRAINING_ENABLE, GLOBAL_FLOW_CONTROL_MODE, PRIORITY_FLOW_CONTROL, UPDATE_DSCP, FULL_DUPLEX_MODE, IPG, PKT_TX_ENABLE, HOST_TX_SIGNAL_ENABLE, AUTO_NEG_FEC_MODE_OVERRIDE | create/remove (breakout), ADVERTISED_SPEED, PORT_SERDES, QoS map/policer/buffer bindings, PFC, per-counter clear, stats_ext READ_AND_CLEAR |
| QUEUE / INGRESS_PRIORITY_GROUP | 8 + 8 per port, G: TYPE, INDEX, PORT, PARENT_SCHEDULER_NODE / PORT, INDEX, BUFFER_PROFILE (null) | | set, stats, scheduler groups |
| VLAN | C/R; C/G VLAN_ID; G MEMBER_LIST, STP_INSTANCE (default); the CPU is a tagged member of every VLAN | LEARN_DISABLE, MAX_LEARNED_ADDRESSES, flood control types, MCAST lookup key types, META_DATA, CUSTOM_IGMP_SNOOPING_ENABLE | stats, flood groups, ACL bindings |
| VLAN_MEMBER | C/R/G; VLAN_TAGGING_MODE UNTAGGED/TAGGED (S re-programs) | | PRIORITY_TAGGED |
| BRIDGE | default .1Q only; G TYPE, PORT_LIST | MAX_LEARNED_ADDRESSES, LEARN_DISABLE, flood control | .1D bridges |
| BRIDGE_PORT | type PORT on a PORT or a LAG (VLAN membership and learning applied to every member), C/R/G; S/G ADMIN_STATE (kept), FDB_LEARNING_MODE (HW/DISABLE/DROP/CPU -> bcm_port_learn) | MAX_LEARNED_ADDRESSES, INGRESS/EGRESS_FILTERING, TAGGING_MODE, limit violation action | SUB_PORT, 1D_ROUTER, 1Q_ROUTER, TUNNEL |
| FDB_ENTRY | C/R/S/G on .1Q VLANs (port or trunk destination): TYPE (STATIC/DYNAMIC), BRIDGE_PORT_ID, PACKET_ACTION (FORWARD/DROP); flush by bridge port / VLAN / type; LEARNED/MOVE/AGED notifications from the SDK's L2 callbacks | META_DATA, ALLOW_MAC_MOVE | .1D (bridge bv_id), flush of static-only on hardware (SDK has no static-only delete) |
| STP | default instance G: VLAN_LIST, BRIDGE_ID, PORT_LIST (empty) | | create, STP ports |
| HOSTIF | type NETDEV on a front-panel PORT = KNET netif (TX to the port) + RX filter on the ingress port (LAG members keep theirs: teamd runs on them); C/G NAME, OBJ_ID, TYPE; C/S/G OPER_STATUS (netdev carrier via /proc/bcm/knet/link), VLAN_TAG (STRIP/KEEP/ORIGINAL: filter re-created), QUEUE (kept) | | FD, GENETLINK, LAG/VLAN objects, packet RX/TX API |
| HOSTIF_TRAP_GROUP | default + C/R/S/G: ADMIN_STATE, QUEUE (kept) | | POLICER (no policer objects) |
| HOSTIF_TRAP | C/R/S/G TRAP_TYPE, PACKET_ACTION, TRAP_PRIORITY, TRAP_GROUP. Programmed: ARP request/response, DHCP, IGMP, MLD, IPv6 ND (switch controls); LACP, LLDP (exact L2 user entry, BPDU flag: CPU only, or discard when not trapped; removed: discard) | all other trap types are kept but **not programmed** (BGP, ... rely on SDK defaults / routes to CPU) | user-defined traps |
| HOSTIF_TABLE_ENTRY | C/R/G (kept): WILDCARD + NETDEV_PHYSICAL/LOGICAL_PORT is what the per-port KNET filters already do | other types/channels kept, not programmed | set |
| VIRTUAL_ROUTER | default only; G ADMIN_V4/V6_STATE, SRC_MAC_ADDRESS | | create (VRFs) |
| ROUTER_INTERFACE (M2) | PORT on a port or a LAG (internal VLAN from 4094 down, port / every member untagged + PVID, L3 intf; neighbors egress to the trunk), VLAN, LOOPBACK (no SDK object); C/S/G SRC_MAC_ADDRESS (my-station entries), MTU; ADMIN_V4/V6_STATE kept | NEIGHBOR_MISS_PACKET_ACTION, V4/V6_MCAST_ENABLE, LOOPBACK_PACKET_ACTION, IS_VIRTUAL, NAT_ZONE_ID, DISABLE_DECREMENT_TTL, ADMIN_MPLS_STATE | SUB_PORT, BRIDGE, MPLS, stats |
| NEIGHBOR_ENTRY (M2) | C/R/G DST_MAC_ADDRESS (S: egress replace), PACKET_ACTION FORWARD/DROP/TRAP, NO_HOST_ROUTE; egress port from the port RIF, or the FDB for VLAN RIFs (CPU until known) | | remove_all, encap index |
| NEXT_HOP (M2) | type IP, C/R/G; uses the neighbor's egress object (CPU until the neighbor exists, then routes are re-pointed) | | other types |
| ROUTE_ENTRY (M2) | default VR, IPv4/IPv6, C/R/S/G PACKET_ACTION (FORWARD/DROP/TRAP), NEXT_HOP_ID = next hop, RIF (connected: to CPU), CPU port (IP2ME) | META_DATA | next-hop groups (ECMP) |
| LAG | SDK trunk (bcm_trunk_*, PSC RTAG 6 = src+dst IP); C/R; G PORT_LIST (LAG_MEMBER ids); C/S/G PORT_VLAN_ID (every member); INGRESS/EGRESS_ACL null only | DEFAULT_VLAN_PRIORITY, DROP_(UN)TAGGED, TPID | SYSTEM_PORT_AGGREGATE_ID, hash/ARS, labels, sampling, ACLs |
| LAG_MEMBER | C/R/G LAG_ID, PORT_ID; C/S/G EGRESS_DISABLE, INGRESS_DISABLE (BCM_TRUNK_MEMBER_*); a joining port takes the LAG's VLANs, PVID, learning and RIF VLAN, a leaving one drops them (PVID 1, learning HW) | WEIGHT | bulk create/remove |
| everything else (ACL, QoS maps, buffers, policers, mirror, tunnels, ...) | | | `sai_api_query` -> NOT_IMPLEMENTED |

Global: `sai_api_initialize/query/uninitialize`, `sai_log_set`,
`sai_object_type_query`, `sai_switch_id_query`, `sai_query_api_version`
(1.19.2), `sai_dbg_generate_dump` (text dump of the object model),
`sai_query_attribute_capability`, `sai_query_attribute_enum_values_capability`
(PORT FEC_MODE/LOOPBACK_MODE, HOSTIF TYPE, RIF TYPE), `sai_query_stats_capability`
(PORT). `sai_object_type_get_availability`, `sai_get_maximum_attribute_count`,
`sai_get_object_count`, `sai_get_object_key`: NOT_SUPPORTED. The bulk-stats /
`query_stats_st_capability` / TAM entry points are deliberately absent, so
syncd's configure leaves them out (VendorSai then uses its null paths).

## Building and testing (no hardware)

```sh
cd ai_re   # all commands with NIXPKGS_ALLOW_UNFREE=1 for the SDK parts
N='let f = builtins.getFlake "path:'$PWD'/nix"; p = import f.inputs.nixpkgs { system = "x86_64-linux"; };
   sonic = import '$PWD'/nix/packages/sonic { pkgs = p; }; libsai = p.callPackage '$PWD'/nix/packages/sai-openbcm { inherit sonic; }; in'
nix build --impure --expr "$N libsai"                      # fake-only libsai + unit tests (checkPhase)
nix build --impure --expr "$N sonic.sairedis-openbcm libsai"
nix build --impure --expr "$N import $PWD/nix/packages/sai-openbcm/test-syncd.nix { pkgs = p; inherit sonic; }"   # VM test (needs kvm)

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
  drop, prefix canonicalisation, in-use checks), LAG (members and disable
  flags, PVID, bridge port + VLAN + FDB on the trunk, join/leave, port RIF +
  neighbor on the trunk, LACP trap, member hostifs), temperature attributes
  and rounding, the temperature publisher (files, failed/implausible read,
  removal), capability table, dump, remove/re-create.
- `test-syncd.nix` (NixOS VM): redis on `/var/run/redis/redis.sock`, syncd
  (`-SUu -z redis_sync`) on the fake, `tests/sai_smoke.py` (the
  sai_smoke.py calls with assertions, plus admin-up of port 0), then ASIC_DB:
  1 switch, 33 ports, 256 queues, 256 IPGs, VLANs 1+100, 32 VLAN members, 32
  bridge ports, 1 router interface, and a `port_state_change` notification.

## Running on the AS7760 (main session)

Prerequisites: the openbcm kmods for the running kernel, the cross-built
`sairedis-openbcm` closure (it pulls in sai-openbcm and libsdk), redis, the
swss-common `database_config.json`, and the factory SDK config
(`config-32x100.bcm` or the 128x25 one). **The config file is not in this
repo**; copy it from the extracted factory image (`nixos-build/vendor-root/etc/knet/`).

1. Nothing else may own the ASIC: `systemctl stop openbcm-asic-temp` (fand
   goes to 100% until syncd's publisher writes the file, see step 3), no
   `bcm.user` running. On the
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

- **Hardware coverage is one smoke test** (2026-11-15, steps above). Expect SDK API
  return codes to need tuning (e.g. FEC controls, port speed changes that need
  FlexPort, `bcm_port_resource` vs `SOC_INFO` lanes).
- **Endianness**: only `sdk_glue.c` converts to SDK types; IPv4 addresses
  are assembled from network-order bytes into host integers (`bcm_ip_t`), MACs
  and IPv6 are byte arrays, so the code is byte-order agnostic. The SDK's own
  BE settings (`SYS_BE_PIO=1 SYS_BE_PACKET=0 SYS_BE_OTHER=1`) are the ones the
  bring-up proved with bcm.user and KNET. SAI and SDK headers are compiled for
  the target, no on-wire structs are exchanged.
- **KNET**: netifs are TX_LOCAL_PORT on the port (bypass switch logic) with the
  port's PVID; RX only for packets the ASIC sends to the CPU, which needs traps
  or CPU VLAN membership. LACP and LLDP are trapped (L2 user entries; "init
  all" already sends 01:80:c2:00:00:0x to the CPU); BGP and other L3
  protocol traps are not programmed (no switch control on this path; would
  need FP rules), so those control-plane protocols are incomplete. Packets
  not taken by a KNET filter are dropped by our RX handler (no SAI packet API).
- **LAG**: `INGRESS_DISABLE` is the SDK's `BCM_TRUNK_MEMBER_INGRESS_DISABLE`:
  frames received on that member are handled as from a plain port (not
  dropped, learned on the port, which has no SAI bridge port, so no FDB
  event). Trunk hashing is fixed (RTAG 6); LAG hash attributes are stubs.
  Not run on hardware or under orchagent yet.
- **orchagent on big-endian**: PortsOrch writes `attr.value.u32` for the
  `sai_uint16_t` PORT_VLAN_ID (port and LAG); sairedis serializes `.u16`,
  which on ppc64 is the upper half, i.e. 0. We reject VLAN 0
  (`INVALID_ATTR_VALUE`); orchagent needs a patch (`.u16 = pvid`).
- **Threads**: SDK linkscan, counter, L2X and RX threads run inside syncd; our
  callbacks only queue. `remove_switch` unregisters callbacks but cannot
  detach the SDK (no clean detach path); a second create in the same process
  re-runs `init all` only (untested on hardware).
- **DMA**: KNET + SDK RX use the BDE DMA pool (`dmasize=16M` factory value);
  untested under load with syncd.
- **Not implemented**: ECMP (next-hop groups), ACL, QoS/buffers/
  schedulers/policers (CoPP has no rate limits), port breakout, warm boot
  (`RESTART_WARM` is only stored), mirror, tunnels, sFlow. orchagent has not
  been run against this SAI; its init will hit some of these as errors.
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
