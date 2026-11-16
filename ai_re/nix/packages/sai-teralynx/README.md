# sai-teralynx

`libsai.so` for the Teralynx 7 (Innovium/Marvell, PCI 1d98:1b58) of the
Celestica CS8210-32X: the [sai-openbcm](../sai-openbcm/README.md) SAI frontend,
**unmodified** (its `src/` is passed in as `FRONTEND`), plus our IFCS backend
in `src/` behind the frontend's backend interface (`backend.h`, the
"openbcm" slot: `SAI_OPENBCM_BACKEND=openbcm`). The vendor SDK (IFCS 0.12.41,
2020, arm64) is **dlopen'ed at run time** from the switch; nothing vendor is
linked, built or stored. Status: passes its tests on x86 against a mock
libifcs; **on the chip (2026-11-16, via `bin/sai-teralynx-smoke`, no syncd yet)**: init,
32 ports, admin/oper state and link events on the 400G loop, adopted netdevs,
temperature, clean teardown ([evidence](../../../devices/sonic-nixos/cs8210-sai-hw-20261116.txt));
everything else below is still unverified there.

**libsai-teralynx-shim.so** (`src/shim.c`, next to libsai, dlopen'ed RTLD_GLOBAL
before libifcs): stubs for the 19 CPython 2.7 functions libifcs references
(else `dlopen` fails), and an aligned copy loop that `ifcs_lib.cpp` writes into
libifcs' `memcpy` GOT entries: libifcs copies to/from the PCI BAR, where current
glibc's aarch64 memcpy faults (SIGBUS); the vendor's glibc 2.25 did not.

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
| `SAI_TERALYNX_NETDEV` | `create` (default): `ifcs_netdev_create` per hostif; `adopt`: rename the SDK's `inno_0_<sysport>` (init yaml with netdev `auto_create`, the vendor `-k` mode) |
| `SAI_TERALYNX_SHUTDOWN` | `deinit` (default): ports down, then `ifcs_deinit(COLD)`; `ports-down`: ports down only |
| `SAI_TERALYNX_IFCS_LOG_NAME` | 2nd argument of `ifcs_init` (the SDK's log name, `$II_ROOT/log/<name>.log`); default `sai-teralynx` |
| `SAI_OPENBCM_TEMP_FILE`, `_TEMP_PROM`, `_DIAG_SOCKET` | frontend meaning (fand temperature file, Prometheus file, diag socket) |

The frontend collects every key through the service table's
`profile_get_next_value` (syncd has it); the backend also asks
`profile_get_value` directly for its own keys.

## Run-time vendor files (on the switch, never in the store)

| file (SDK dir) | sha256 | use |
|---|---|---|
| `libifcs.so` | `388031e7eb2443d77d89da826084976859bf92db19f9733be19b1c0b603c0d17` | dlopen'ed (`RTLD_NOW|RTLD_GLOBAL`), 71 symbols ([src/ifcs_api.h](src/ifcs_api.h)); a missing one fails init naming it |
| `libAAPL.so` | `cf4e848498398d4dca083156d9819bfda4c006eebf92c66729a2cdd40d2c41a6` | libifcs dependency, plus jansson/libyaml/zlib, via `LD_LIBRARY_PATH` |
| `cel_configs/isg.yaml` / `isg_R01.yaml` (board id 0x2) | `377e3b97…` / `c1f85dd7…` | `SAI_TERALYNX_CONFIG_APPEND` |
| `cel_cmds/*.cmd` | e.g. `misc_config_32.cmd` `c8847694…` | `SAI_TERALYNX_INIT_CMDS` |

Interop values (names, enum values, layouts) were read from the SDK's
`pyctypes/ifcs_ctypes.py` (sha256 `ba980c0b…`) and checked against the DWARF of
`innovium.user`; [src/ifcs_api.h](src/ifcs_api.h) says which, with static_asserts
(also on aarch64). Local evidence (git-ignored):
`devices/cs8210-32x/analysis/sai-teralynx/` (`innovium-user-main.txt`,
`layouts-innovium-user-dwarf.txt`, `vendor-cmds-parse.txt`, `pahole-libifcs-7.0.1.txt`).

## Init and shutdown

Exactly the vendor's `innovium.user` (disassembly): `ifcs_init(config,
<log name>, IFCS_BOOT_COLD, &num_nodes)`, `ifcs_node_create(0, 0, NULL)`, one
attr via `ifcs_attr_t_init` / `_id_set(0 = NODE_ATTR_ENABLE)` /
`_value_data_set(1)`, `ifcs_node_attr_set(0, 1, &attr)`. Then: every port-map
devport admin-down, the init commands, log / link scan / L2 event callbacks,
one STP instance (all ports FORWARDING), one mc_dest per port, VLAN 1 with
the CPU. Shutdown: all ports admin-down first, callbacks off, netdevs deleted,
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
| admin, link | devport `ADMIN_STATE`, `LINK_STATUS` | V |
| link events | `ifcs_linkscan_event_register_notify` -> queue | I (struct from pyctypes) |
| speed get / set | `SPEED`; set = configured speed only, else NOT_SUPPORTED | V / - |
| FEC | `FEC_MODE`: RS keeps the yaml's KR/KP, else KP for >= 200G or PAM4 lanes, KR otherwise; FC | V attr, I mapping |
| autoneg, loopback | `AUTO_NEG`; MAC -> PCS, PHY -> PMA loopback | V attr, I mapping |
| frame max (MTU) | sysport `MTU` (L2 MTU) = SAI MTU + 22, max 9290 | I |
| PVID, learning | sysport `DEFAULT_CVID`, `LEARN_ENABLE` (drop/cpu modes NOT_IMPLEMENTED) | V |
| counters, clear | `ifcs_devport_stats_get` (CLI form: count ALL), `_clear` | V |
| netdev | `ifcs_netdev_create` (name, devport, MAC, VLAN tag) or adopt; keep_tag = `VLAN_TAG` | I / V (adopt = knet.py) |
| netdev carrier | no IFCS call: libifcs drives it from link scan | I |
| VLAN | L2VNI(vid) + STP + MDG flood group of mc_dests, CPU in every VLAN; untagged = sysport `EGRESS_UNTAGGED_VLAN_ID` (one per port) | V (knet.py) |
| FDB add / delete | `ifcs_l2_entry_create` MAC_L2VNI key, `ENTRY_DEST`, `ENTRY_TYPE`; discard = `FWD_POLICY` drop | V / I |
| FDB flush | walk `ifcs_l2_entry_get_all`, check type/dest, delete one by one | V calls |
| FDB events | `ifcs_l2_event_register_notify` (learned/moved/aged) | I |
| aging | node `L2_ENTRY_AGING_INTERVAL`, seconds | I (units) |
| VRF | `IFCS_HANDLE_L3VNI(vrf+1)`, created on first use; up to 255 | V form |
| L3 interface | `ifcs_intf_create` SVI on the VLAN's L2VNI (port RIFs: the frontend's internal VLAN) | I (sample uses L3PORT) |
| my-station | no IFCS object (an intf terminates its own MAC) | I |
| next hop | `ifcs_nexthop_create` intf + dest MAC + sysport; update in place | V / I |
| routes, hosts | `ifcs_route_entry_create` IP_DEST_L3VNI key; NEXTHOP + FWD forward | V |
| DROP route | `FWD_POLICY` drop, no NEXTHOP | I |
| TRAP route (ip2me, glean) | `FWD_POLICY` drop + `CTC_POLICY` copy to one hostif trap (queue 0) | I (Marvell SAI does drop+ctc) |
| ECMP | `ifcs_ecmp_create` + `member_add`, repeats = `WEIGHT` | I |
| temperatures | node `MASTER_LAST_TEMP` + `REMOTE0..3` (float C) | V (scripts/temp.py) |
| chip limits | node ACL priority range, `NUMBER_OF_ECMP_GROUPS`; 64 paths/group (ours); buffer size 0 (unknown) | V / - |
| table usage | prefixes/hosts from our count vs [fib-capacity.md](../../../devices/cs8210-32x/fib-capacity.md) (24,576 TCAM, no ILPM); nexthop/L2/ECMP `*_usage_get` | I |
| diag shell | read-only `ports`, `devport <id>`, `temp`, `help`; else NOT_SUPPORTED | |
| **LAG** | **NOT_IMPLEMENTED** (members must leave every L2VNI first; LAG handle use unknown, switchd/DESIGN.md U13) | |
| **ACL / field processor** | **NOT_IMPLEMENTED** (CoPP then logs "traps run unpoliced") | |
| policers | kept in the backend only, no IFCS meter (nothing could reference one) | |
| protocol traps (`trap_set`) | NOT_SUPPORTED: IFCS hostif traps are drop/miss reasons; frames reach the CPU via flood groups and TRAP routes | |

## Tests

`make check` (native, also the nix build's checkPhase):
- `tests/test_ifcs.cpp` drives this libsai through the public SAI API with
  `tests/mock_ifcs.c` as libifcs (records calls): refusals (no port map,
  bad lib path, no init yaml) before anything is loaded; the exact init call
  sequence; merged config; parsed init commands in order and the skipped ones;
  ports/lanes, speed, admin, FEC (KP/KR choice, kept when already RS), MTU; link event -> port notification; stats
  and clear; hostif -> netdev; VLAN + untagged member; learned FDB event ->
  notification; static FDB + flush; aging; default DROP routes v4/v6; port RIF,
  neighbor, next hop, route, ip2me route, route kind change; LAG refused;
  temperatures; remove_switch order (ports down, unregister, deinit). 127 checks.
- sai-openbcm's `tests/test_sai.cpp`, unchanged, against this library's
  objects (frontend + fake backend): 2353 checks.
- Built natively with nix (`callPackage ./sai-teralynx { inherit sonic; }`) and
  the backend compiled for aarch64 (static_asserts hold). `TEST_IFCS_DUMP=1`
  prints the IFCS calls.

## Only the chip can tell

Every I row above, and: whether `ifcs_netdev_create` works with this init
yaml (create mode; adopt is the path the vendor stack runs today), which
netdev/ring yaml keys it needs, and whether carrier follows link; the SDK's
default L2 state before VLAN config (PVID, VLAN 1; DESIGN.md U1); whether
link scan needs enabling; L2 event struct and units of the aging interval;
TRAP routes reaching the netdevs; SVI on the internal port-RIF VLAN; route
`attr_set`; ECMP weights; MTU semantics; stats `count = ALL` form on other
counters; that `ifcs_deinit` returns cleanly. LLDP/LACP on a port with no
VLAN and no RIF cannot reach the CPU (no protocol traps).
