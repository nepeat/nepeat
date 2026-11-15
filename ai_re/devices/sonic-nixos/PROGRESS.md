# sonic-nixos — SONiC's switch stack as native NixOS services

Plan (living doc): "NixOS-native SONiC switch stack — build plan" (Claude
Docs; adapted to this tree by suggestion cards on 2026-11-14).
Code: [ai_re/nix/packages/sonic](../../nix/packages/sonic/default.nix); modules
will go in `ai_re/nix/modules/switch/sonic/`. First hardware target: the
Accton AS7760-32X ([its answers](../edge-core-7760-32x/sonic.md)).

## Status (2026-11-15)

- **Real ASIC through SONiC's syncd:** the thin SAI ([sai-openbcm](../../nix/packages/sai-openbcm/README.md))
  drives the BCM56930 on the AS7760: switch create, 32 front-panel ports from
  the SDK (lanes match the factory port map), ports left admin-down, VLAN 100
  and a router interface created, ASIC_DB populated ([transcript](sai-openbcm-hw-20261115.txt)).
  Not yet: swss/orchagent, traffic through SAI-created objects, LAG/ACL/QoS.
- **One SDK process per unit** (2026-11-15): the switch boots with
  `openbcm-asic-temp` holding `/run/openbcm-unit0.lock` for fan control;
  `systemctl stop openbcm-asic-temp` before syncd/`bcm.user` (fans go to 100%
  meanwhile). ASIC temperature over SAI is not implemented yet.

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
- **Next:** hardware test of sai-openbcm, then swss/orchagent (`--with-sai=vs`), then swss;
  `/usr/share/swss` path shim (Lua scripts are loaded from that absolute path).
- **Open:**
  - `sonic-db-cli PING` (TCP to every instance in database_config.json) failed
    on the switch while unix-socket access worked. Not diagnosed: could be the
    unresolvable `redis_chassis.server` instance, or a TCP/port problem on BE.
  - No SAI for this ASIC on PowerPC exists (research below). Until that is
    solved, the hardware target runs the stack with the virtual SAI only.
- **Not in the repo (review later for what can be public):** in use now, read
  from the extracted factory image under the AS7760's gitignored
  `nixos-build/vendor-root/etc/knet/`: `config-32x100.bcm` (port map, lane
  maps, polarity; Accton copyright header in its scripts) and, not used yet,
  its `rc-*.soc`, `portled.soc` (LED program) and pre-emphasis scripts. Other
  port modes exist there too (128x25, 128x10, mixed). Also still private: factory `bcm.user` (SDK 6.4.8 binary), the SDK config
  / port map and LED programs inside the factory NOS image, Broadcom SAI
  `.deb`s (amd64 only; referenced by URL, never stored).

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
