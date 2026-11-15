# sonic-nixos — history

Entries moved verbatim from [PROGRESS.md](PROGRESS.md) (newest first).

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
