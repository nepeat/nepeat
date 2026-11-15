# sonic-nixos — history

Entries moved verbatim from [PROGRESS.md](PROGRESS.md) (newest first).

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
