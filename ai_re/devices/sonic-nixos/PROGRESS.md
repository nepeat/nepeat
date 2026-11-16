# sonic-nixos — SONiC's switch stack as native NixOS services

Plan (living doc): "NixOS-native SONiC switch stack — build plan" (Claude
Docs; adapted to this tree by suggestion cards on 2026-11-14).
Code: [ai_re/nix/packages/sonic](../../nix/packages/sonic/default.nix), NixOS
module [ai_re/nix/modules/switch/sonic](../../nix/modules/switch/sonic/README.md)
(`services.sonic`). First hardware target: the
Accton AS7760-32X ([its answers](../edge-core-7760-32x/sonic.md)).

## Status (2026-11-15)

- **Runs** on the AS7760 at boot ([config](../../nix/configs/as7760/configuration.nix),
  [board](../../nix/modules/boards/as7760/default.nix), [module](../../nix/modules/switch/sonic/README.md)):
  syncd on [sai-openbcm](../../nix/packages/sai-openbcm/README.md) (OpenBCM 6.5.27, config.bcm
  generated from the Nix ports, cages 1/32 split 4x25G), orchagent + managers, FRR
  (bgpd/staticd/bfdd, FPM, bgpcfgd), pmon, LLDP; 38 ports, 0 restarts, fand fed by syncd.
- **Verified on the chip** (lab loop Ethernet1/1 <-> Ethernet32/1, 25G, both ends here;
  entry below): links, LACP, VLAN + SVI ([lacp-vlan](sonic-as7760-lacp-vlan-20261115.txt));
  counters, crash recovery, reload ([counters](sonic-as7760-counters-recovery-20261115.txt),
  [platform-frr-recovery](sonic-as7760-platform-frr-recovery-20261115.txt)); `show platform`,
  transceivers, LLDP, FRR (same file); VRFs, ASIC routing via a next hop and ECMP, iBGP,
  static routes ([l3-ecmp-bgp](sonic-as7760-l3-ecmp-bgp-20261115.txt)); ingress ACL, CoPP
  policers ([acl-copp](sonic-as7760-acl-copp-20261115.txt)); software BFD ([bfd](sonic-as7760-bfd-20261115.txt)).
- **Test / debug:** `openbcm-diag <cmd>` = SDK diag shell inside syncd
  ([how](vrf-ecmp-test.md#debug-sdk-state-while-syncd-owns-the-chip-2026-11-15)); `sonic-nix-diff`
  = CONFIG_DB vs Nix ([ownership](config-ownership.md)); VM tests in `nix/modules/switch/sonic/tests`
  (vs: vm, vm-frr, vm-bfd, vm-platform, vm-metrics; sai-openbcm fake: vm-openbcm-fake, -l3, -acl), sai-openbcm
  unit tests + test-syncd. Switch scripts: [vrf-ecmp](vrf-ecmp-test.md), [acl-copp](acl-copp-test.md),
  [bfd](bfd.md), [platform](../edge-core-7760-32x/sonic-platform.md).
- **Metrics:** `sonic_*` from redis via the node exporter (producer `sonic`, [sonic-metrics](../../nix/packages/sonic/metrics/README.md)),
  Grafana `ai-re-sonic`; VM-tested, not yet deployed to the AS7760 ([evidence](sonic-metrics-20261115.txt)).
- **Change it:** the AS7760's `deploy.sh`; a changed orchagent restarts syncd too (links flap).
- **Open:** ARP passes its 600 pps CoPP cap (818/s); not built: QoS (low priority), buffers,
  egress ACLs, ACL ranges/redirect/mirror, runtime breakout, warm reboot; `show acl` fails
  (pyangbind), no `aclshow`; untested: traffic between real hosts, line rate, 100G, FEC,
  IPv6/multihop BFD, port LEDs by eye; `sonic-db-cli PING` over TCP fails (unix socket fine);
  bfdd leak (json-c 0.19, [bfdd-leak](bfdd-leak.md)) and `FEATURE|bgp` CoPP traps fixed,
  VM-tested, not deployed; vm-frr's "syncd killed" checks fail since 76d6818.
- **CS8210-32X (Teralynx 7, aarch64):** our SAI `sai-teralynx` (this frontend + an IFCS backend
  on the vendor libifcs, loaded at run time) initialises the chip and brings the 400G loop up
  (direct libsai run, no syncd yet); the stack is wired but not run there:
  [cs8210-32x/sonic.md](../cs8210-32x/sonic.md).
- **Not in the repo:** factory SDK configs, rc/LED/pre-emphasis scripts and `bcm.user`
  (AS7760 `nixos-build/vendor-root/`, unused now), Broadcom SAI `.deb`s (amd64, by URL).

## 2026-11-16 — CS8210-32X: own Teralynx SAI, first runs on the chip

- No arm64 Teralynx SAI is published (98 packages, all amd64: [list](cs8210-sai-search-20261115.txt)),
  so [packages/sai-teralynx](../../nix/packages/sai-teralynx/README.md) = sai-openbcm's frontend
  (unchanged; built from its `src/` by path) + an IFCS 0.12.41 backend, `libifcs.so` dlopen'ed from
  the vendor SDK on the switch. Tests: 127 backend checks on a mock libifcs + sai-openbcm's 2353.
- Board layer [boards/cs8210](../../nix/modules/boards/cs8210/sonic.nix): 32x8-lane port map, IFCS
  init yaml generated from the SONiC ports (equal to the vendor generator's output for every mode),
  runtime vendor tuning files, read-only pmon; machine glue in `nepeat/nix` `nixos/module/cs8210/sonic`.
- Shared-module changes, AS7760 byte-neutral (`nixosConfigurations.as7760` toplevel drvPath
  `z05gsrr0…-nixos-system-as7760-26.11.20261111.d4f25a3.drv` before and after each): `ports.nix`
  stride = lanes of the widest cage (8 for QSFP-DD), `services.sonic.allowUnusedLanes`,
  `services.sonic.pmon.args` (CS8210: xcvrd `--skip_cmis_mgr`).
- On the chip ([evidence](cs8210-sai-hw-20261116.txt)): libifcs needs CPython symbols (stubbed) and
  **faults in current glibc's memcpy on the PCI BAR** (SIGBUS; its memcpy import now goes to an
  aligned copy loop). Then: init, 32 ports, 400G loop up in ~4 s, netdevs adopted, temperature,
  clean teardown. Open list: [cs8210-32x/sonic.md](../cs8210-32x/sonic.md#before-the-first-sonic-run).

## 2026-11-15 (night) — bfdd leak = json-c 0.19; FEATURE|bgp seed

- The soak's bfdd growth (~2.9 MB/min) is json-c 0.19, not FRR: its new
  `json_object_put()` frees non-empty containers inside `assert()`, compiled
  out under NDEBUG, so every JSON tree leaks; bfdsyncd's `show bfd peers json`
  every 200 ms triggered it (bgpd leaks the same way under bgpmon, slower).
  Found with the new `sonic.routing.malloc-trace` on the switch, stacks
  resolved with an unstripped rebuild (.text identical). FRR now links
  `sonic.routing.json-c` (+ upstream f291fa81c6, 3aa996b5b6, 0c3a5a1994)
  ([bfdd-leak](bfdd-leak.md), [before/after](bfdd-leak-jsonc-20261115.txt)).
- `tests/vm-bfdd-leak.nix` runs bfdsyncd: stock json-c reproduces (+2901
  kB/min, the switch's rate), patched flat (0 bytes in 10 min per phase). The
  earlier flat VM runs had no JSON poller ([those runs](bfdd-leak-vm-20261115.txt));
  the switch's "bfdsyncd stopped" check was undone at once by orchagent's
  Upholds (use `systemctl mask --runtime --now`).
- `frr.nix` seeds `FEATURE|bgp` (init_cfg values): coppmgrd installs the
  bgp/bgpv6 CoPP traps (vm-bfd 99/99 incl. COPP_TABLE and the ASIC_DB hostif traps).
- Regressions: vm-bfd 99/0, vm 92/0, vm-platform 31/0; cross-build in budget
  (2566160384 bytes; libfrr's RUNPATH has the patched json-c). vm-frr fails 4
  "syncd killed" checks also on a clean HEAD export: from 76d6818 (syncd PartOf
  orchagent), not from this.

## 2026-11-15 (late) — SONiC metrics and the Grafana dashboard (VM only)

- `services.sonic.metrics` ([metrics.nix](../../nix/modules/switch/sonic/metrics.nix)): textfile producer
  [sonic-metrics](../../nix/packages/sonic/metrics/README.md), stdlib Python reading redis directly; on by
  default with the node exporter (the AS7760 has it). Ports (name + alias), counters, PortChannels, BGP, BFD,
  LLDP, optics, ACL/CoPP, routes, conflicts, unit restarts; `sonic_up 0` without redis.
- Unit test (real redis + fake dump, 128 ports: 6 ms of redis reads on x86) and VM test `tests/vm-metrics.nix`
  (PortChannel + BGP/BFD to a peer, Vrf-test, LLDP, fake optics, ACL, a conflict, a killed unit, redis
  stopped): pass; producer run 0.17 s in the VM. Existing VM tests: same drvPath; AS7760 closure +28 KiB
  ([evidence](sonic-metrics-20261115.txt)). Dashboard `ai-re-sonic` (tools/grafana), SONiC panel on the lab overview.
- Found: bgpmon covers the default VRF only (VRF neighbors: config, no state; fixed 2026-11-16, routing patch 0006); no bgp CoPP trap (Status, Open);
  vs SAI gives no ACL counter values. Switch check: [README](../../nix/packages/sonic/metrics/README.md#check).

## 2026-11-15 (evening, night) — the stack on the chip: platform, FRR, recovery, routing, ACL/CoPP, BFD

- Platform CLI, LLDP self-loop, FRR units + FPM, `config interface ip add/remove`,
  teamd respawn, orchagent kill -> syncd restart with all 38 netdevs back,
  `config reload` without a saved file: OK ([evidence](sonic-as7760-platform-frr-recovery-20261115.txt)).
  PSU 2 "NOT OK" = no power cord (owner), so the PSU bit layout matches.
- Routing took three tries: VRF objects in the chip but every unicast frame to
  the router MAC dropped ([try 1](vrf-hw-try1-20261115.txt)); after per-port L3
  enable + ARP in the my-station entry (bb06d32) ARP and the VRF binding work,
  but no route in hardware: the public config's `l3_alpm_enable=2` on a libsdk
  without ALPM ([try 2](vrf-hw-try2-20261115.txt)); key dropped (3982c39): VRF
  pings 0 % loss, TTL-63 frames routed by the ASIC via one next hop and via an
  ECMP group, negative check, static routes ([evidence](sonic-as7760-l3-ecmp-bgp-20261115.txt)).
- iBGP across the VRFs sat in Connect: the NixOS firewall dropped TCP 179 ->
  `services.sonic.frr.openFirewall` (bcc8f9c); then the learned prefix reaches
  APPL_DB and the ASIC.
- ACL drop rule on Ethernet1/1: 100 % loss, counter 40 packets, removal
  restores; CoPP IP2ME at 100 pps holds a 66k pps flood to 110/s; ARP exceeds
  its 600 pps cap (818/s), open ([evidence](sonic-as7760-acl-copp-20261115.txt)).
- BFD: the first deploy restarted only orchagent (changed unit) and left no
  KNET netdevs -> syncd `PartOf` orchagent (76d6818). Then: sessions Up at
  300 ms x3; with BFD packets dropped BGP Down in 0.69 s, session 0.84 s;
  static routes with bfd leave APPL_DB 1.0-1.6 s after and return; bfdd +
  bfdsyncd ~0 % CPU. Port shutdown: Down 4.5-4.7 s after the step's T0, which
  includes the CLI call ([evidence](sonic-as7760-bfd-20261115.txt)).
- Fans stayed at the minimum throughout (pwm 136, ASIC 41-43 °C).

## 2026-11-15 — ACL, policers and CoPP policing in sai-openbcm (fake + VM)

- New in sai-openbcm ([README](../../nix/packages/sai-openbcm/README.md#acl-and-copp-field-processor)):
  POLICER on SDK meters (sr/tr_tcm, packets/bytes, colour drops); a trap
  group's POLICER = CoPP field entries per trap (protocol match, one shared
  meter per SAI policer; copied traps lose only the CPU copy, CPU-only traps
  are dropped; IP2ME/BGP/SSH/SNMP/BFD via "routed to the CPU egress");
  ingress ACL_TABLE (SDK field group), ACL_ENTRY (field entry on the bound
  ports), ACL_COUNTER, ACL_TABLE_GROUP(_MEMBER), PORT/LAG INGRESS_ACL, the
  capability answers AclOrch reads. Not done: egress ACLs, ACL ranges,
  redirect/mirror actions, QoS (maps, schedulers; owner: low priority).
- Unit tests 2355 checks (new: tests/test_acl.inc). VM test
  vm-openbcm-fake-acl.nix: run 1 found AclOrch adding FIELD_ACL_RANGE_TYPE
  to every ingress table (refused -> now kept, ranges themselves
  unsupported), run 2 that ACL counters need `counterpoll acl enable`; run 3
  28/28 ([evidence](sonic-acl-fake-vm-20261115.txt)). sdk_glue.c compiles
  against the SDK headers, the cross libsai links (every bcm_field_* /
  bcm_policer_* symbol in libsdk.so, [cross](sonic-acl-cross-20261115.txt)).
  Regressions pass with syncd rebuilt on the new libsai: vm-openbcm-fake 86,
  vm-openbcm-fake-l3, test-syncd, the ACL VM again ([runs](sonic-acl-regress-20261115.txt)).
- Found on the way, not fixed (not in this change's paths): `show acl ...`
  fails (acl-loader imports pyangbind, missing from sonic-utilities' closure),
  `aclshow` is not installed; `config acl add table -p` wants SONiC port
  names (aliases refused); counters.nix's comment says sai-openbcm has no ACL
  counters (it has now).
- Hardware unknowns and the test plan: [acl-copp-test.md](acl-copp-test.md),
  flood helper [copp_flood.py](copp_flood.py).

## 2026-11-15 — software BFD (bfdd, BGP + static routes), VM only

- FRR built with bfdd (no SONiC bfdd patch exists); `switch/sonic/bfd.nix`:
  `sonic-frr-bfdd`, `sonic-staticroutebfd`, `sonic-bfdsyncd`, bgpcfgd bound to
  bfdd, `SYSTEM_DEFAULTS|software_bfd` seeded (bgpcfgd's BfdMgr), vs SAI
  told `SAI_VS_BFD_OFFLOAD_SUPPORTED=false`, UDP 3784/3785/4784 opened like BGP.
- Upstream gaps found and closed (patches in packages/sonic/routing): bgpcfgd
  ignores BGP_NEIGHBOR `bfd` (only frrcfgd renders it) -> template patch +
  live toggle; BfdMgr makes sessions passive by default (two SONiC ends never
  come up) -> active unless `async_passive`; staticroutebfd hardcodes 50 ms
  -> `bfd.staticRoute` (300 ms x3); nothing reports software session state
  (staticroutebfd waits for STATE_DB BFD_SESSION_TABLE, which only BfdOrch's
  hardware path writes) -> bfdsyncd (ours, polls bfdd's vty).
- VM test `tests/vm-bfd.nix`: 96/96 on the last two runs ([evidence](sonic-bfd-vm-20261115.txt),
  history in its header). Fixed on the way: orchagent's BfdMonitorOrch threw
  on bfdsyncd's extra STATE_DB fields (bfdsyncd writes BfdOrch's set only);
  staticroutebfd (1 s to stop) ordered after bgpcfgd made `config reload`
  race (vm-frr caught it; no ordering now); a peer with a configured and a
  BGP session to one address flaked (bfdd wildcard match; test links split).
  Regressions pass: vm-frr 86, vm 92, vm-platform 31, eval.nix; fake/fake-l3
  unchanged (frr off).
- Cross-builds into the AS7760 system, +1.2 MB ([cross](sonic-bfd-cross-20261115.txt)).
- Known limits (README): static BFD and BGP BFD to the same next hop make two
  bfdd sessions; staticroutebfd skips VLAN interfaces; `config route add`
  has no bfd option; orchagent warns "BFD session for <ip> not found" on each
  state change (VNET code path, same upstream with hardware BFD).

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
- Hardware try 1 ([evidence](vrf-hw-try1-20261115.txt)): VRF objects in the chip,
  but every unicast frame to the router MAC dropped (IPv4, ARP replies).
  Fix (unverified): L3 enabled on all front-panel ports at SDK init (PORT_TAB
  V4L3/V6L3_ENABLE were never set), RIF VLAN L3 enable, my-station entry also
  terminates ARP. Debug hook: `openbcm-diag <cmd>` (SAI_OPENBCM_DIAG_SOCKET, on
  in the AS7760 board); checks in [vrf-ecmp-test.md](vrf-ecmp-test.md#debug-sdk-state-while-syncd-owns-the-chip-2026-11-15).
- Hardware try 2 ([evidence](vrf-hw-try2-20261115.txt)): L3 enable, ARP and the
  VRF binding work; no route was in hardware (`l3 defip show` empty): the
  public config's `l3_alpm_enable=2` with a libsdk built without ALPM. The
  AS7760 config.bcm now drops it (routes in the DEFIP TCAM) and syncd refuses
  such a config ([builds](l3-ecmp-builds-20261115.txt)).
- Routes, pings across the VRFs and ECMP: proven that night (entry above).

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
- **Untested on the switch at the time** (it ran that evening; its
  `l3_alpm_enable=2` was the routing bug, dropped in 3982c39). New vs every earlier hardware run: the public
  file's globals (L3 ALPM, `bcm_num_cos=10`, timeouts, ...) and the
  pre-emphasis at init. `sai.sdkConfig.settings` overrides globals (`null`
  removes). New files need `git add -N` before a git-flake deploy.

Older entries: [HISTORY.md](HISTORY.md).
