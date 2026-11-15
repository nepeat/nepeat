# sonic-nixos — SONiC's switch stack as native NixOS services

Plan (living doc): "NixOS-native SONiC switch stack — build plan" (Claude
Docs; adapted to this tree by suggestion cards on 2026-11-14).
Code: [ai_re/nix/packages/sonic](../../nix/packages/sonic/default.nix), NixOS
module [ai_re/nix/modules/switch/sonic](../../nix/modules/switch/sonic/README.md)
(`services.sonic`). First hardware target: the
Accton AS7760-32X ([its answers](../edge-core-7760-32x/sonic.md)).

## Status (2026-11-15)

- **VRF, ECMP, flex counters** (2026-11-15, fake-tested, not on the switch):
  kernel NET_VRF, `sonic-vrfmgrd`, sai-openbcm virtual routers (SDK VRFs)
  and next-hop groups (SDK ECMP groups), real ECMP capacities;
  FLEX_COUNTER_TABLE|PORT seeded; `show ... -i` alias lists. Switch test
  (needs a reboot for the kernel): [vrf-ecmp-test.md](vrf-ecmp-test.md).

- **Platform, LEDs, LLDP: VM-tested, not on the switch yet** (2026-11-15
  entry): pmon (xcvrd, thermalctld monitor-only, psud, syseepromd) over an
  AS7760 `sonic_platform`, `show platform fan|temperature|psustatus|syseeprom`,
  `show interfaces transceiver`, the public LED program at SDK init, LLDP
  (lldpd + lldpmgrd + lldp_syncd, `show lldp neighbors|table`). 35/35 VM checks
  on a fake board incl. an LLDP self-loop ([VM](sonic-platform-vm-20261115.txt)); cross-builds
  (+1.5 MB, [cross](sonic-platform-cross-20261115.txt)). Switch test and the unverified PSU
  bits: [sonic-platform.md](../edge-core-7760-32x/sonic-platform.md).

- **SONiC runs at boot on the AS7760** (generated config.bcm with cages 1/32
  split 4x25G, kernel with bridge/VLAN/team): syncd, orchagent and all
  managers up with 0 restarts, 38 ports; the lab loop links at 25G
  (Ethernet1/1 <-> Ethernet32/1); LACP PortChannels on both ends come up
  (members selected, LAG members in the ASIC) once one end gets its own
  system MAC (same-switch loop: libteam refuses to aggregate with itself);
  VLAN 100 on a PortChannel with an SVI reaches the kernel bridge and the
  ASIC ([evidence](sonic-as7760-lacp-vlan-20261115.txt)). fand gets the ASIC
  temperature from syncd. Not yet: data-plane traffic between hosts,
  ECMP/ACL/QoS/policers.

- **FRR routing stack + crash recovery, VM-tested, not on the switch yet**
  (2026-11-15 entry): FRR 10.7 (zebra/mgmtd/bgpd/staticd) with zebra's FPM to
  fpmsyncd and bgpcfgd (SONiC's default "separated" mode, config from
  CONFIG_DB), vtysh on PATH (`config interface ip remove`, `show ip route`,
  `show ip bgp summary`). VM: eBGP to a peer over a LACP PortChannel, learned
  prefixes in APPL_DB, ASIC_DB and the kernel, offload acks, static routes from
  CONFIG_DB; teamd respawn, orchagent failure -> syncd restart, `config
  reload` without a saved file ([VM, 86 checks](sonic-frr-vm-20261115.txt)).
  Cross-builds into the AS7760 system, 2.56 of 3 GiB budget ([cross](sonic-frr-cross-20261115.txt)).

- **SONiC control plane on the real chip** (2026-11-15, `test` activation,
  old kernel): `services.sonic` with sai-openbcm: syncd, orchagent,
  portsyncd, portmgrd, intfmgrd, teammgrd active with 0 restarts,
  `PortInitDone`, 32 KNET netdevs, `show interfaces status` in alias mode,
  `config interface startup Ethernet3/1` reaches the chip (ce2 admin up). fand
  keeps the ASIC temperature from syncd. vlanmgrd needs the new kernel
  (bridge/VLAN/team), which needs a reboot: held until the console is back;
  `configs/as7760` keeps SONiC disabled until then. Not yet: traffic, VLAN and
  PortChannel on the chip, ECMP/ACL/QoS/policers (unsupported in sai-openbcm).

- **SDK config from the Nix ports** (2026-11-15, not on hardware yet): the
  AS7760's `config.bcm` is generated from the SONiC ports (public AS7712-32X
  values, per-cage breakout) and is syncd's `SAI_INIT_CONFIG_FILE`; the lab
  config splits cages 1 and 32 4x25G for the QSA loop. The hand-copied factory
  file is no longer used (2026-11-15 entry).

- **Control plane as NixOS services, VM-tested** (x86_64, virtual SAI):
  `services.sonic` runs redis, syncd, orchagent, port/intf/vlan/nbr/team
  managers and syncers, the show/config CLI in alias mode
  (`Ethernet<cage>/<sub>`), with the [config ownership](config-ownership.md)
  rules (enforce/seed/unmanaged, conflicts in `/etc/issue`, motd and
  `sonic-nix-diff`). All 92 VM checks pass, incl. a 4x25G split, alias
  ranges, VLAN, LACP PortChannel to a peer VM, reboot and `nixos-rebuild switch`
  ([VM](sonic-module-vm-20261115.txt), [eval](sonic-module-eval-20261115.txt)).
  The same test on sai-openbcm's fake backend (what the AS7760 runs, minus the
  ASIC) passes too: 86 checks, 3 N/A (no breakout in the SAI)
  ([log](sonic-openbcm-fake-vm-20261115.txt)).
  Cross-builds into the AS7760 system with the openbcm syncd, +240 MiB
  ([cross](sonic-module-cross-20261115.txt)). **Not on the switch yet**: its
  kernel lacks bridge/8021q/team/dummy, and it needs the board wiring.

- **Real ASIC through SONiC's syncd:** the thin SAI ([sai-openbcm](../../nix/packages/sai-openbcm/README.md))
  drives the BCM56930 on the AS7760: switch create, 32 front-panel ports from
  the SDK (lanes match the factory port map), ports left admin-down, VLAN 100
  and a router interface created, ASIC_DB populated ([transcript](sai-openbcm-hw-20261115.txt)).
  Since then also through syncd: ASIC temperature attributes and LAG (2 members,
  egress disable) ([transcript](sai-lag-temp-hw-20261115.txt)). Not yet:
  swss/orchagent on the switch (first try aborted in AclOrch init; fixed in
  sai-openbcm and VM-tested on the fake, 2026-11-15 entry; rerun pending),
  traffic through SAI-created objects, ACL/QoS.
- **swss packaged** (`sonic.swss`: orchagent, port/intf/vlan/team managers,
  portsyncd, teamsyncd, neighsyncd, fdbsyncd, ...; plus `sonic.libteam` with
  SONiC's teamd patches and `sonic.dash-api`): native build passes upstream's
  gtest suites, cross-builds for ppc64 BE, and the same tests pass on BE under
  qemu except 4 test-only artifacts ([evidence](swss-build-20261115.txt)).
  Runs in the VM test above; not yet on the switch. Big-endian fixes carried
  as patches (2026-11-15 entry).
- **One SDK process per unit** (2026-11-15): the switch boots with
  `openbcm-asic-temp` holding `/run/openbcm-unit0.lock` for fan control;
  `systemctl stop openbcm-asic-temp` before syncd/`bcm.user` (fans go to 100%
  meanwhile, until syncd's profile sets `SAI_OPENBCM_TEMP_FILE`, which keeps
  fand's ASIC sensor alive).

## Status (2026-11-14, earlier)

- **Phase:** 0 done (pins + target answers), Phase 1 started.
- **Works:** `sonic-swss-common` builds natively (x86_64) and **cross-builds for
  ppc64 big-endian**; on the AS7760 itself, against a cross-built redis:
  `sonic-db-cli` HSET/HGETALL, Python `Table` set/get and `ConfigDBConnector`
  return correct data. First evidence the stack is not little-endian-only.
- **ASIC access (OpenBCM 6.5.27, route 1 of the research):** the SDK's BDE
  kernel modules build for ppc64 BE / Linux 7.3.4 (`nix/packages/openbcm`) and
  load on the AS7760: `linux-kernel-bde` enumerates `14e4:b930` rev 0x12 as
  unit 0 (iProc PAXB init OK, 32 MB DMA pool), `linux-user-bde` reports
  "Interrupt mode CMICd". The SDK userspace (`bcm.user` diag shell) builds for
  ppc64 BE (1887 objects, Tomahawk only) and **attaches on the switch**:
  `Chip BCM56930_B1, Driver BCM56960_A0`; `CMIC_DEV_REV_ID = 0x12b930` decodes
  correctly. With the factory 32x100G `config.bcm`: **`init all` passes, all 32
  front-panel ports (ce0–ce31) plus xe0/xe1 enumerate**, a port comes up at
  100G in MAC loopback, an L2 entry writes and reads back, and 5 packets sent
  from the CPU are counted out and back in on the port (640 bytes each way).
  [Transcript](openbcm-bringup-20261114.txt). SDK kernel modules and bcm.user
  are Nix packages now (`as7760-openbcm-kmods`, `as7760-bcm-user`;
  [notes](../../nix/packages/openbcm/README.md)), verified on the switch.
- **Also works:** packets to the CPU (RX), the KNET kernel module (front-panel
  ports as Linux netdevs, TX and RX, DMA interrupts), and **real links**: two
  breakout lanes (front port 1 and 32) link at 10G and carry frames both ways
  between two Linux netdevs, ASIC counters matching, no errors
  ([transcript](openbcm-real-links-20261114.txt)).
- **25G:** the same loop (QSFP28 breakout leg 1 into port 32 through the
  user's QSFP/SFP+ adapter) links at 25G without FEC in the factory 128x25
  mode and passes frames both ways; the eye is about half the 10G height.
- **Not tested:** 100G on real cables, FEC,
  the other 126 lanes, pre-emphasis/LED scripts, SDK interrupts (factory
  config polls), throughput, long runs.
- **sairedis** (syncd, libsairedis, libsaivs = virtual SAI, SAI 1.19 headers)
  builds natively and cross-builds for ppc64 BE. The SAI metadata generator
  needed its XML::Simple mode with current doxygen (see the derivation).
- **SAI pipeline runs on the AS7760** with the virtual SAI: Python client →
  libsairedis → redis → syncd → libsaivs, all ppc64 BE: switch create, 32
  ports with lanes/speed, default VR, VLAN 100, a router interface; ASIC_DB
  holds the expected objects (33 ports, 672 queues, ...). Script:
  [sai_smoke.py](sai_smoke.py) with [vsprofile.ini](vsprofile.ini); run
  `syncd -SUu -z redis_sync -p vsprofile.ini` against a redis on
  /var/run/redis/redis.sock first.
- **Thin SAI over OpenBCM** (`nix/packages/sai-openbcm`): written and tested
  against its fake backend and through syncd in a VM; cross-builds; **not yet
  run on the AS7760** (2026-11-15 entry).
- **Next:** hardware test of sai-openbcm, then swss/orchagent against it.
- **Open:**
  - `sonic-db-cli PING` (TCP to every instance in database_config.json) failed
    on the switch while unix-socket access worked. Not diagnosed: could be the
    unresolvable `redis_chassis.server` instance, or a TCP/port problem on BE.
  - No SAI for this ASIC on PowerPC exists (research below). Until that is
    solved, the hardware target runs the stack with the virtual SAI only.
- **Not in the repo (review later for what can be public):** in the
  extracted factory image under the AS7760's gitignored
  `nixos-build/vendor-root/etc/knet/`: `config-*.bcm` (no longer used since
  2026-11-15: Nix generates the SDK config from public values) and, not used,
  its `rc-*.soc`, `portled.soc` (LED program) and pre-emphasis scripts. Other
  port modes exist there too (128x25, 128x10, mixed). Also still private: factory `bcm.user` (SDK 6.4.8 binary), the SDK config
  / port map and LED programs inside the factory NOS image, Broadcom SAI
  `.deb`s (amd64 only; referenced by URL, never stored).

## 2026-11-15 — pmon platform API, front-panel LEDs, LLDP (VM only)

- `nix/modules/boards/as7760/sonic_platform`: SONiC platform API 2.0 for the
  AS7760 (fans from hwmon + fand's state file, read-only; 4 LM75 + ASIC;
  PSUs from CPLD#1 0x02 with the AS7712/AS7716 bit layout, unverified;
  32 cages via CPLD#1 + as7760-qsfp's bus map; ONIE TLV from NOR hw-info).
  No thermal manager, so thermalctld never runs a fan policy; no LED or PWM
  writes. Unit test on `fake_board.py` (13 tests). Details and the switch
  test: [sonic-platform.md](../edge-core-7760-32x/sonic-platform.md).
- `as7760-qsfp`: added combined I2C transfers (`read_atomic`/`write_atomic`,
  I2C_RDWR) and a per-bus lock around page-select reads; the platform uses
  them so xcvrd/psud/the tool cannot interleave register-pointer writes. The
  tool's own `read()` is unchanged (still two transfers).
- `switch/sonic/pmon.nix`, `lldp.nix`; packages `sonic.platform` (pmon
  daemons from sonic-platform-daemons, platform CLI scripts, lldpd without
  SNMP + SONiC patch 0001, lldpmgrd, lldp_syncd from sonic-dbsyncd, new pin
  `dbsyncd` = buildimage's submodule 0cfeda31). lldpshow/lldp_syncd call
  lldpctl directly (no container).
- LEDs: `SAI_OPENBCM_RC_FILE` = public AS7712 `led_proc_init.soc` (= factory
  portled.soc); `led auto on` makes the SDK linkscan feed link state to the
  LED processor, so no ledd.
- Found on the way: the board's TLV platform name is
  `powerpc-accton_as7760_32x-r0`, while `services.sonic.platform` is
  `powerpc64-…` (nothing reads the TLV one; left as is).
- VM test `tests/vm-platform.nix`: 35/35 ([evidence](sonic-platform-vm-20261115.txt)); vm.nix
  (92 PASS) and vm-openbcm-fake.nix (86 PASS) still pass with the new modules
  imported; eval.nix builds.

## 2026-11-15 — VRFs, ECMP, flex counter seed (fake + VM; switch test designed)

- Kernel: NET_VRF needs IP_ADVANCED_ROUTER -> IP_MULTIPLE_TABLES,
  NET_L3_MASTER_DEV and IPV6_MULTIPLE_TABLES (Linux 7.3.4 Kconfig); all set,
  NET_VRF as a module (vrf.ko in the as7760 closure).
- `services.sonic.vrf.enable` (vrfmgrd), `services.sonic.flexCounters` (PORT
  seeded: syncd polls from boot), sonic-utilities patch 0004 (alias comma
  lists/ranges in `show ... -i`).
- sai-openbcm: VIRTUAL_ROUTER = SDK VRF (RIF VLAN bound via `l3a_vrf`, host
  and route entries per VRF, route leaking), NEXT_HOP_GROUP(_MEMBER) = SDK
  ECMP groups (`bcm_l3_egress_ecmp_*`, BCM_L3_MULTIPATH routes), ECMP
  capacities from the SDK instead of 0. Unit tests 2005/0; new VM test
  (VRF + RouteOrch next-hop group + counters + CLI): 37/0; regression VMs and
  eval pass ([builds](l3-ecmp-builds-20261115.txt), [VM](sonic-l3-fake-vm-20261115.txt)).
- Port counters (task 2): fine on the chip once polled (coordinator, 2026-11-15).
- Open: none of the SDK VRF/ECMP calls has run on the BCM56930 yet.

## 2026-11-15 — FRR routing stack (FPM, bgpcfgd) and crash recovery

- `services.sonic.frr` ([frr.nix](../../nix/modules/switch/sonic/frr.nix), on by
  default), packages `sonic.routing` (nixpkgs FRR 10.7.1 = SONiC's release, with
  sonic-frr patches 0008/0013/0017/0026 + one local dplane_fpm_nl patch; the
  docker-fpm-frr templates; bgpcfgd/bgpmon). SONiC uses its own
  `dplane_fpm_sonic` (needs libnexthopgroup); we use FRR's `dplane_fpm_nl`
  with the same encoding: without 0008 fpmsyncd drops every default-VRF route
  (table 254 read as a VRF ifindex), without the local patch zebra never sees
  the offload acks (routes stay "queued").
- bgpcfgd configures no BGP_NEIGHBOR until DEVICE_METADATA `type`,
  `BGP_DEVICE_GLOBAL|STATE` (now seeded) and Loopback0 exist.
- Upstream sonic-utilities bug fixed (python patch 0005): in alias mode
  `show ip bgp summary` crashed (BGP JSON fed to the route-alias rewrite).
- Crash recovery from the hardware run ([recovery.nix](../../nix/modules/switch/sonic/recovery.nix),
  [hardware evidence](sonic-as7760-counters-recovery-20261115.txt)): (a) teamd
  respawned with `-o` by `sonic-teamd-watch` (IP/members kept, other
  PortChannels untouched); (b) orchagent failure restarts syncd + everything
  below; every syncd start flushes APPL/ASIC/COUNTERS DBs as swss.sh does
  (without it a restarted syncd's APPLY VIEW failed and orchagent aborted in a
  loop until start-limit); (c) `config reload` with no saved file loads the
  Nix base (python patch 0001).
- VM: [sonic-frr-vm-20261115.txt](sonic-frr-vm-20261115.txt) (86 checks, 0
  failed); vm.nix (92) and vm-openbcm-fake.nix (86, 3 N/A) still pass with FRR
  and recovery in. Cross: [sonic-frr-cross-20261115.txt](sonic-frr-cross-20261115.txt)
  (FRR's installed headers/libtool files had pulled the build machine's
  x86_64 clippy helper, 235 MiB, into the switch closure; now stripped and a
  disallowed reference). Open: frrcfgd/unified mode, BFD; static routes show
  protocol `196` in APPL_DB (iproute2's rt_protos has no name; SONiC alike);
  an APPL_DB flush also drops lldp_syncd's LLDP_ENTRY_TABLE until its next
  change (lldp is not restarted with syncd).

## 2026-11-15 — config.bcm generated from the Nix ports (per-cage breakout)

- `services.sonic.sai.sdkConfig.generator` (generic: a board function
  `{ ports, settings } -> file`; its file is the default `sai.initConfigFile`).
  AS7760: [sdk-config.nix](../../nix/modules/boards/as7760/sdk-config.nix) over
  [openbcm/configs](../../nix/packages/openbcm/README.md#configs) (Tomahawk rules
  from the OpenBCM 6.5.27 source: logical = physical + 0/1/3/5 per pipe, lanes
  per speed, one VCO per core, per-lane keys split as the SDK reads them).
  Base: sonic-buildimage's AS7712-32X 32x100G file (already pinned). Per-key
  decisions in sdk-config.nix (management ports kept, factory's 67/101
  portmaps dropped as SDK no-ops, driver current left at the SDK default).
- Checked locally against the factory 32x100G and 128x25G configs (not in the
  repo): every portmap, lane map and polarity key agrees (except the two
  no-op portmaps); the public pre-emphasis equals the factory script's
  pre/main/post on all 128 lanes in both modes. Not reproduced: the factory
  script's post2 taps (no config.bcm key), 10G-specific taps.
- Lab config: cages 1 and 32 split 4x25G, every subport listed. Tests
  ([log](sdk-config-20261115.txt)): pinned output, split = 4 valid portmaps,
  other modes build, impossible layouts fail, the system's sai.profile points
  at the generated file; module eval test passes; VM tests unchanged
  (same derivations). Sonic-off system drvPath unchanged; sonic-on cross-builds.
- **Untested on the switch.** New vs every earlier hardware run: the public
  file's globals (L3 ALPM, `bcm_num_cos=10`, timeouts, ...) and the
  pre-emphasis at init. `sai.sdkConfig.settings` overrides globals (`null`
  removes). New files need `git add -N` before a git-flake deploy.

## 2026-11-15 — orchagent runs on sai-openbcm (fake backend VM test)

- Switch symptom (orchagent aborts in AclOrch init on ACL_ENTRY_MIN/MAX_PRIORITY)
  reproduced in a VM: new `tests/vm-openbcm-fake.nix` runs the whole
  services.sonic stack and vm-test.py on sai-openbcm's fake backend (32 cages
  x 4 lanes, AS7760 port map, `asic_type` broadcom). Baseline libsai: same
  abort, PortInitDone never set ([log](sonic-openbcm-fake-vm-20261115.txt), run 1).
- sai-openbcm now answers what orchagent reads at init and CRM polls; per
  attribute from the swss source, table in the
  [sai-openbcm README](../../nix/packages/sai-openbcm/README.md#what-orchagent-reads).
  Real (SDK at run time on openbcm): ACL entry priority range, TOTAL_BUFFER_SIZE
  (TH MMU), AVAILABLE_IPV4/6 ROUTE/NEXTHOP/NEIGHBOR/FDB. Honest zeros:
  ECMP groups/members (no NHG API, so RouteOrch never creates groups), ACL
  action count/stage lists. Still NOT_SUPPORTED: STP instances, NAT, hashes,
  ACL resource lists (orchagent survives each). 795 unit checks.
- Fake backend: `SAI_OPENBCM_FAKE_NETDEV=tap` (hostif = TAP netdev, like KNET)
  and `SAI_OPENBCM_FAKE_WIRE` (CPU path of a port to a host NIC), so swss
  and teamd have netdevs and LACP reaches the peer VM.
- Result: 86 checks pass, 3 N/A (4x25G split, lanemap.ini, split-cage range:
  no breakout in the SAI), 0 fail: ports, admin state by alias, VLAN +
  member, RIF/IP, PortChannel with LACP to the peer and ping across it, the
  enforce/seed conflict flow (Ethernet1/1 speed enforced by `fieldModes`
  here), reboot, rebuild, reload, restartOnStop (run 2 in the log).
- Left for the switch: CoPP policers are NOT_IMPLEMENTED (6 ERRORs + a missing
  `/usr/bin/sai_failure_dump.sh` per boot, no rate limits); the fake's wire
  is only the CPU path, so no ASIC forwarding is tested; the openbcm values
  above are compiled (cross build) but not yet read on the chip.
- Cross-built for the AS7760 (README commands): libsai `7ccqhpcz…-sai-openbcm-powerpc64…`,
  `rbd14dx5…-sonic-sairedis-sai-openbcm-powerpc64…` (= the board's
  `services.sonic.sai.package`). The vs variant still passes (92/92).

## 2026-11-15 — services.sonic: control plane + config ownership, VM-tested

- New: `nix/packages/sonic/python` (sonic-py-common, sonic-config-engine,
  sonic-yang-models + sonic-yang-mgmt on CESNET libyang-python 3.1.0 with
  SONiC's patches, pinned in `sources.json`; sonic-platform-common,
  sonic-utilities) and `nix/modules/switch/sonic` ([README](../../nix/modules/switch/sonic/README.md)).
  YANG validation is real (libyang 3.13.6 from nixpkgs; SONiC's NOEXTDEPS
  patch is unused by the Python side): Nix base at build time, merged config
  at boot.
- swss-common: the Python bindings now carry the `CFG_*_TABLE_NAME` macros
  (sonic-utilities imports them); changes every SONiC store path (intended).
- sonic-utilities patches: 0001 `config reload` -> `sonic-nix reload`;
  0002 alias mode (`portchannel member add/del` wrote the alias as key;
  `run_command(return_cmd=True)` returned None). Caveat review of the
  `Ethernet<cage>/<sub>` aliases: [naming-caveats.md](naming-caveats.md).
- Gotchas found: orchagent needs `/zmq_swss` (crash otherwise); show
  portchannel needs `tlm_teamd`; Linux 802.3ad needs a slave speed (virtio
  has none: `ethtool -s` in the peer VM); kernel messages hide agetty's
  issue on tty1 in test VMs.
- syncd always `Conflicts=openbcm-asic-temp` (both open the SDK; one process
  per unit); without `SAI_OPENBCM_TEMP_FILE` a NixOS warning says fans lose
  the ASIC sensor while syncd runs. `sai.restartOnStop` starts it again when
  syncd stays stopped (10 s check, skipped during restarts and reloads).
- Follow-ups (same day): SONiC names from the first lane (2x50G cage 1 =
  Ethernet0/Ethernet2; my first formula used the subport, spec corrected);
  hostname (and time zone, if set) mirrored into DEVICE_METADATA as enforced
  keys instead of dropped; sonic-utilities patch 0003: alias ranges
  `Ethernet1/1-4`, `Ethernet1/3-3/1`, and alias filters resolved to names in
  `config interface startup/shutdown` (comma lists of aliases wrote alias keys).

## 2026-11-15 — sonic-swss packaged; big-endian fixes in orchagent and SAI meta

- New: `nix/packages/sonic/{swss,libteam,dash-api}`, wired into
  `sonic/default.nix`; `dash-api` pin added to `sources.json` (buildimage
  submodule SHA). [Evidence](swss-build-20261115.txt); BE test runner
  [swss-be-tests.nix](swss-be-tests.nix).
- **BE bugs fixed (patches, upstreamable):**
  - swss 0001: SAI attribute values written through a union member of the
    wrong width. PORT/LAG `PORT_VLAN_ID` (u16 written as u32: every PVID 0 on
    BE, found by the sai-openbcm work), plus NAT zone id, WRED weight, mirror
    TOS, MPLS pop count, DTEL latency/hop count, three DASH attrs, path-tracing
    timestamp type. Found with `swss/check-sai-value-types.py` (header @type
    vs `.value.<member>`). Not fixed (no BE effect or not used here): enums via
    u32, u32 via s32, bufferorch u32/u64 on pool type/mode (checker false positive:
    .value set before .id), p4orch policer attrs.
  - sairedis 0001: SAI `saiserialize.c` IPv4/IPv6 mask (de)serializers byte-swap
    unconditionally; on BE every non-trivial mask failed ("has holes"), so
    route/ACL entries with prefixes could not be serialized. Changes the vs
    `sairedis` (and sairedis-openbcm) store paths: intended.
  - swss 0002 (tests only): fpmsyncd test helpers (stack overflow; `nl_attr_put8`
    taking a u16; LE-only be64 construction).
- swss-common: `cfg_schema.h` (CFG_*_TABLE_NAME macros, empty upstream without
  libyang) generated from the pinned sonic-yang-models by a text parser
  (`gen-cfg-schema.py`); Lua dir overridable (`SWSS_LUA_DIR`). sairedis installs
  the meta headers swss needs.
- Deviations (all commented in the derivations): C++17, protobuf 3.21 (current
  protobuf needs abseil/C++20), libteam from the v1.31 tag without D-Bus/ZMQ,
  shell commands at store paths (iptables/conntrack/brctl/ndisc6 from PATH),
  countersyncd (Rust) and the warm-reboot Python helpers not built.
- Open: run orchagent + managers in a NixOS VM (vs SAI) and on the AS7760;
  4 remaining BE test failures are test artifacts (see evidence); my first
  attempt ran native and cross builds plus 13 qemu test runs at once and ran
  the build host out of memory. Run one build at a time.

## 2026-11-15 — thin SAI over OpenBCM (sai-openbcm), not yet on hardware

- New package [sai-openbcm](../../nix/packages/sai-openbcm/README.md): an
  Apache-2.0 `libsai.so` (SAI 1.19) with two backends behind one interface:
  `openbcm` (OpenBCM via the new `libsdk` output of
  `nix/packages/openbcm`) and an in-memory `fake`. M1 (switch, ports with
  lanes from the SDK, VLAN/bridge/FDB, KNET hostifs, traps) and M2 (port/VLAN
  router interfaces, neighbors, next hops, routes) are implemented; LAG,
  ECMP, ACL, QoS are not. Supported/stubbed table in its README.
- `sonic.sairedis-openbcm libsai`: syncd linked against it (configure's
  version check given a cross-compiling branch). The vs `sairedis` is
  unchanged (same drvPath native and cross).
- Tests without hardware: 449 unit checks against the fake; a NixOS VM test
  (redis + syncd -SUu on the fake + an asserting copy of sai_smoke.py) passes:
  32 ports, lanes 1-4, VLAN 100, router interface created, a port oper-up
  notification reaches the Python client, ASIC_DB has 33 ports / 256 queues /
  2 VLANs / 32 VLAN members / 32 bridge ports / 1 RIF.
- Cross builds for the AS7760 succeed: libsdk.so, libsai (openbcm), syncd.
- Safety choices: ports are disabled right after SDK init (SAI default
  admin-down), VLAN 1 keeps the SDK's default membership for orchagent to
  manage, stale KNET netifs/filters are destroyed at init, no
  `SAI_INIT_CONFIG_FILE` means no init, the fake refuses to run where BDE
  devices exist.
- Hardware test procedure: the README's "Running on the AS7760". Nothing was
  run on the switch for this entry.

Older entries: [HISTORY.md](HISTORY.md).
