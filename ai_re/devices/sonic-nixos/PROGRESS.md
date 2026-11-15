# sonic-nixos — SONiC's switch stack as native NixOS services

Plan (living doc): "NixOS-native SONiC switch stack — build plan" (Claude
Docs; adapted to this tree by suggestion cards on 2026-11-14).
Code: [ai_re/nix/packages/sonic](../../nix/packages/sonic/default.nix); modules
will go in `ai_re/nix/modules/switch/sonic/`. First hardware target: the
Accton AS7760-32X ([its answers](../edge-core-7760-32x/sonic.md)).

## Status (2026-11-14)

- **Phase:** 0 done (pins + target answers), Phase 1 started.
- **Works:** `sonic-swss-common` builds natively (x86_64) and **cross-builds for
  ppc64 big-endian**; on the AS7760 itself, against a cross-built redis:
  `sonic-db-cli` HSET/HGETALL, Python `Table` set/get and `ConfigDBConnector`
  return correct data. First evidence the stack is not little-endian-only.
- **ASIC access (OpenBCM 6.5.27, route 1 of the research):** the SDK's BDE
  kernel modules build for ppc64 BE / Linux 7.3.4 (`nix/packages/openbcm`) and
  load on the AS7760: `linux-kernel-bde` enumerates `14e4:b930` rev 0x12 as
  unit 0 (iProc PAXB init OK, 32 MB DMA pool), `linux-user-bde` reports
  "Interrupt mode CMICd". Userspace SDK (bcm.user) for ppc64 BE: not built yet.
- **Next:** sairedis with the virtual SAI (`--with-sai=vs`), then swss;
  `/usr/share/swss` path shim (Lua scripts are loaded from that absolute path).
- **Open:**
  - `sonic-db-cli PING` (TCP to every instance in database_config.json) failed
    on the switch while unix-socket access worked. Not diagnosed: could be the
    unresolvable `redis_chassis.server` instance, or a TCP/port problem on BE.
  - No SAI for this ASIC on PowerPC exists (research below). Until that is
    solved, the hardware target runs the stack with the virtual SAI only.
- **Not in the repo (review later for what can be public):** nothing copied
  yet. Candidates when hardware work starts, all still only in the AS7760's
  gitignored `dumps/`: factory `bcm.user` (SDK 6.4.8 binary), the SDK config
  / port map and LED programs inside the factory NOS image, Broadcom SAI
  `.deb`s (amd64 only; referenced by URL, never stored).

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
