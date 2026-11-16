# sonic-nixos — history

Entries moved verbatim from [PROGRESS.md](PROGRESS.md) (newest first).

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

## Status block as of 2026-11-15 evening (replaced 2026-11-15 night)

- **BFD on the AS7760** (software, FRR bfdd; Tomahawk has no usable offload;
  [evidence](sonic-as7760-bfd-20261115.txt)): sessions Up across the VRFs over
  the lab cable at 300 ms x3; with BFD packets dropped, the session is Down
  in 0.84 s and BGP in 0.69 s (vs the 90 s hold time); static routes with
  bfd=true leave APPL_DB within 1.0-1.6 s and come back; bfdd + bfdsyncd < 1 %
  CPU. **ACL + CoPP policers** on the field processor work on the chip
  ([evidence](sonic-as7760-acl-copp-20261115.txt)); ARP exceeds its 600 pps
  cap (818/s, likely the flood copy to the CPU), QoS unsupported. Found and
  fixed on the way: a NixOS activation restarting only orchagent left no
  KNET netdevs (syncd is now PartOf orchagent).

- **Software BFD, VM-tested, not on the switch yet** (2026-11-15 entry,
  [bfd.md](bfd.md)): FRR bfdd; BGP neighbors with `bfd=true` drop in
  0.66-0.90 s on loss/link down (hold time 90 s), over a PortChannel and a
  port in a VRF; static routes with `bfd=true` (staticroutebfd) leave and
  re-enter APPL_DB with their session; `show bfd summary`. No ASIC offload:
  orchagent finds none and hands sessions to software, no errors
  ([VM, 96 checks](sonic-bfd-vm-20261115.txt), [cross](sonic-bfd-cross-20261115.txt)).
  Switch test: bfd.md.

- **Routing through the AS7760's chip works** (2026-11-15,
  [evidence](sonic-as7760-l3-ecmp-bgp-20261115.txt)): VRFs (pings across
  VRF 0 / Vrf-test over the lab cable, 0% loss), frames routed by the ASIC
  (TTL 63) via one next hop and via an ECMP group (SDK ECMP egress object, 2
  members), iBGP between the VRFs over the cable with the learned prefix in
  APPL_DB and the ASIC, static routes from FRR in the ASIC. Fixes that got
  there: per-port L3 enable + ARP in the my-station entry (sai-openbcm),
  no ALPM in config.bcm (our libsdk has none: routes were silently never
  looked up), BGP allowed through the host firewall on switch interfaces.
  Debug: `openbcm-diag <cmd>` runs SDK diag commands inside syncd.

- **ACL + CoPP policers on the field processor** (2026-11-15, fake-tested,
  not on the switch): sai-openbcm implements POLICER (SDK meters), trap
  group policers as CoPP field entries, ingress ACL tables/entries/counters/
  table groups bound to ports and LAGs; orchagent's CoPP no longer logs
  policer errors, `config acl` / CONFIG_DB rules reach the SAI
  ([VM, 28 checks](sonic-acl-fake-vm-20261115.txt)). Switch test:
  [acl-copp-test.md](acl-copp-test.md). QoS stays unsupported.

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
  ACL/policers on the chip (fake-tested), QoS.

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
  PortChannel on the chip, ECMP/ACL/policers (since added, see above), QoS.

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

## Status block as of 2026-11-14, kept below the newer one until 2026-11-15 night

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

## 2026-11-14 (night, last) — module EEPROMs, 25G, SDK as Nix packages

- The far ends were a loop: leg 1 of the QSFP28 breakout in port 1 goes into
  port 32 through a QSFP-to-SFP adapter. Port 32's EEPROM read "garbled"
  because the pca9548 muxes kept their last channel selected, so two modules
  answered at 0x50 at once. Fixed with `i2c-mux-idle-disconnect` in the device
  tree. New tool `as7760-qsfp` (table / details / JSON; SFF-8636 and
  SFF-8472; port-to-mux mapping from the factory device table).
- 25G on that loop works in the 128x25 mode (earlier failure: port 32 lanes
  were 10G-only in the 124x25-4x10 mode, not the adapter).
- `nix/packages/openbcm/default.nix`: pinned sparse fetch + patches; bcm.user
  built by Nix attaches on the switch with Nix-built modules.

Older entries (2026-11-14: Phase 0, first cross-built component, OpenBCM
kernel modules, diag shell, first packets and links): [HISTORY.md](HISTORY.md).

## 2026-11-14 (night, latest) — CPU RX, KNET netdevs, first real links

- RX to CPU: L2 entry pointing at cpu0, looped packets arrive in the diag
  shell's packet watcher with correct port/VLAN metadata.
- KNET ported to Linux 7 (five API fixes + dev_addr_set; patch 0001). Netdev
  for a port transmits and, once the SDK's RX is started, receives.
- User cabled front port 1 (QSFP28→4×SFP28 passive breakout, 3 m, read from
  its EEPROM on i2c bus 24) and front port 32 (4×10G breakout; EEPROM on bus
  47 reads garbled). Used the factory `124x25-4x10` mode. Kept each port in
  its own VLAN (ports removed from VLAN 1 first) so nothing could bridge.
  No link with autoneg or at 25G (with/without FEC); SerDes diag showed signal
  but no lock on one lane; at 10G both lanes linked (eyes open). Frames with
  ethertype 0x88b5: 5/5 each way between the two netdevs.
- Mistake worth remembering: KNET keeps netifs across SDK restarts, so IDs are
  not 1,2 unless stale ones are destroyed first; my first run delivered frames
  to the wrong netdev. Also a second SDK instance launched through a FIFO did
  not start; the test silently ran against the old one.
- The Falcon SerDes reports die temperature (41–42 °C here): a usable ASIC
  temperature source for fan control later.
- QSFP EEPROMs of modules inserted after boot are not bound (at24 probes at
  boot only); read with i2ctransfer for now. Module reset/low-power lines via
  the CPLD are not handled.

## 2026-11-14 (night, later) — full SDK init and first packets

- User confirmed no front-panel ports are cabled and approved reading the
  factory port map. Factory image has per-mode SDK configs and init scripts;
  used `config-32x100.bcm` unchanged (written for SDK 6.4.8, accepted by 6.5.27).
- `init soc`, then `init all`: no errors. `ps`: 32 ce ports + 2 xe.
  `port ce0 lb=mac sp=100000`: link up 100G. `l2 add`/`l2 show`: OK.
  `tx 5 pbm=ce0`: TPKT=5, RPKT=5, 640 bytes each direction.
- Factory loads the BDE with `himem=0 maxpayload=128 dmasize=16M`; we used
  dmasize=32M and the default maxpayload (driver logs "exceeds device
  capability (256)"). Align with the factory values next.

## 2026-11-14 (night) — OpenBCM diag shell attaches to the ASIC from ppc64 BE

- New SDK userspace platform `ppc64be` (patch 0002): SDK's PowerPC endian
  flags + 64-bit word-size flags, dynamic link, no -Werror, `-fpermissive` for
  GCC 16. The SDK makefiles hard-code `/bin/echo` etc. (`fixpaths.pl`).
- On the AS7760: `BDE dev 0 (PCI), Dev 0xb930, Rev 0x12, Chip BCM56930_B1,
  Driver BCM56960_A0`, `SOC unit 0 attached`. `getreg CMIC_DEV_REV_ID` =
  `0x12b930`. This is route 1 of the research working further than any
  public report found for an e6500 host. OpenMDK (route 2) not needed so far.

## 2026-11-14 (late) — OpenBCM kernel modules load; PCIe aperture bug found

- First `insmod linux-kernel-bde` machine-checked the kernel (panic, automatic
  reboot) on the first MMIO read of the ASIC; a plain userspace read of BAR2
  did the same. Cause was not the SDK: the Linux device tree placed the PCIe
  controller's CPU aperture at 0xc_6000_0000, but U-Boot opens the local access
  window (LAW) for it at 0xc_4000_0000, and Linux never programs LAWs. Fixed in
  the AS7760 device tree; BAR reads now work (`000c0f00` at CMIC offset 0).
  Nothing had ever read ASIC memory under mainline before, so this was latent.
- The panic also left the freshly copied modules as zero-length files (ext4,
  not yet flushed): "insmod: Invalid parameters" with no kernel message. Sync
  after copying test binaries to the switch.
- Kernel-module port to Linux 7: timer/MAX_ORDER renames and explicit
  module_init/exit (patch 0001 in `nix/packages/openbcm/patches`). Built with
  the SDK's own PowerPC endian settings (SYS_BE_PIO=1, SYS_BE_OTHER=1, BE_HOST).

## 2026-11-14 — Phase 0 and first cross-built component

- Pinned sonic-buildimage `47fcc8d` and its submodule SHAs in
  `nix/packages/sonic/sources.json` (the only place a SONiC rev appears).
- libnl: no fork. SONiC patches libnl 3.7 for RTA_NH_ID; nixpkgs' 3.12 has it
  upstream as `rtnl_route_get_nhid`, so consumers get a `-D` alias instead.
- swss-common divergences from debian/rules, recorded in the derivation: unit
  tests not built (C++14 vs nixpkgs gtest needing C++17, and they need redis),
  `-Werror` relaxed for GCC 16, Python include/lib/install paths fixed, headers
  installed by hand, `database_config.json` installed under `share/`.
- Upstream has `device/accton/x86_64-accton_as7712_32x-r0`: the x86 sibling of
  this switch. A public reference for port naming and the SDK config layout.
- Research (Sonnet agent, web only): [research-ppc-sai-20261114.md](research-ppc-sai-20261114.md).
