# BCM56930 SDK and Linux 7 investigation — 2026-11-14

## Result

Newer public sources exist. The strongest route for this switch is classic
OpenBCM with a matched userspace/kernel interface, using current SONiC GPL
module maintenance as a porting reference. Linux 7 is a realistic target:
**three SDK 6.5.35 kernel modules compile and link against Linux 7.3.4 after
small compatibility changes**, including successful modpost and BTF generation.

This is an **x86_64 compile-only result**, not a PowerPC build or hardware test.
No modules were loaded. Serial, power, flash, and SD were untouched in this
investigation. The switch remains at the previously established U-Boot prompt.
The SDK is not yet a validated Linux 7 switching stack for this board.

## Chip and current SDK evidence

- Existing hardware PCI ID: `14e4:b930`; historical diagnostic logs identify
  `BCM56930_B1`. The bundled `bcm.user` strings identify **sdk-6.4.8**.
- The binary is 32-bit big-endian PowerPC, dynamically linked to uClibc.
- OpenBCM 6.5.27 defines device `0xb930`, B1 revision `0x12` in
  [devids.h](https://github.com/Broadcom-Network-Switching-Software/OpenBCM/blob/6d2330ce1b4fc49d681cffcf09af408661be4c62/sdk-6.5.27/include/soc/devids.h).
- More importantly, its [ESW driver](https://github.com/Broadcom-Network-Switching-Software/OpenBCM/blob/6d2330ce1b4fc49d681cffcf09af408661be4c62/sdk-6.5.27/src/soc/esw/drv.c#L2932)
  assigns BCM56930 to `SOC_INFO_CHIP_TOMAHAWK` and dispatches its port setup
  and queues through Tomahawk functions. This is actual source dispatch,
  not just an ID definition. Board-specific port/PHY settings still matter.

## Candidates

| Source | Evidence | Limitation |
| --- | --- | --- |
| [OpenBCM 6.5.27](https://github.com/Broadcom-Network-Switching-Software/OpenBCM) | Public full SDK source; explicit BCM56930 Tomahawk dispatch | Repository archived; custom Broadcom license for SDK core, GPL portions for kernel modules; Linux 7 port still required |
| [SONiC saibcm-modules, sdk-6.5.35-xgs](https://github.com/sonic-net/saibcm-modules/tree/sdk-6.5.35-xgs) | Newer public kernel-side source, GPLv2 option in core module files; explicit BCM56930 PCI matches | Not a complete SDK userspace release; PowerPC operation and ABI matching untested |
| [OpenNSA 6.5.17](https://github.com/Broadcom-Network-Switching-Software/OpenNSA/blob/master/README.md) | Published API and kernel sources | Included userspace library is x86; not a usable PowerPC SDK replacement |
| [SDKLT snapshot](https://github.com/opennetworkinglab/SDKLT) | Tomahawk source, API definitions Apache 2.0 | Archived; mixed licenses, not uniformly Apache; device table includes BCM56960 variants but no BCM56930 entry found |

OpenBCM snapshot: `6d2330ce1b4fc49d681cffcf09af408661be4c62`.
SONiC module snapshot: **`a1a9879e6aded405bd9f03462926d4223e33de8b`**, dated
2026-11-11, commit title “Update kernel version references to 6.12.94 (#43)”.
This is the fetched branch tip, newer than the separately inspected SONiC
buildimage submodule pin `b9b38791bc5fd81f846789677ca873345fbf2048`.

The [OpenBCM license](https://github.com/Broadcom-Network-Switching-Software/OpenBCM/blob/master/Legal/LICENSE)
is a Broadcom-specific license; do not describe the complete SDK as GPL.
The SDKLT [repository description](https://github.com/opennetworkinglab/SDKLT)
and [chip-code license](https://github.com/opennetworkinglab/SDKLT/blob/master/Legal/Broadcom_Switch_Software_license)
also distinguish API licensing from the rest of the implementation.
No fully open, ready-to-run BCM56930/PowerPC/Linux-7 package was verified.

## Linux 7.3.4 compilation

Sources were built through a small Kbuild harness rather than the vendor
makefiles, which assume `/usr/bin/perl`, internal compiler paths and older
kernel compilation conventions. Unmodified sources failed against 7.3.4.

Changes:

1. [linux7-compat.h](linux7-compat.h): map legacy timer helpers to
   `timer_delete`, `timer_delete_sync`, and `timer_container_of` for Linux 7+.
2. [linux7-module-api.patch](linux7-module-api.patch): use explicit
   `module_init`/`module_exit` declarations instead of magic function names.
3. Same patch: use `strscpy` for the KNET interface name; kernel 7 headers no
   longer expose the old `strncpy` call used there.

Built successfully:

- `linux-kernel-bde.ko`
- `linux-user-bde.ko`
- `linux-bcm-knet.ko`

Compiler: native x86_64 GCC 16.2.0 matching the kernel. BTF tool: pahole 1.32.
Artifacts live under ignored `build/kbuild-7.3.4/`; they are **not switch binaries**.
Logs retain initial failures, including compiler/tool setup failures and an
intermediate patch-placement mistake corrected before the successful build.
Final successful build: [kbuild-linux-7.3.4-success.txt](kbuild-linux-7.3.4-success.txt).
Reproduction using the saved harness: [probe-linux7-repro.txt](probe-linux7-repro.txt).
No warning/error suppression or unresolved-symbol bypass was used.

Reproduce from this directory, inside the project nix devshell:

```sh
git clone --branch sdk-6.5.35-xgs https://github.com/sonic-net/saibcm-modules.git sources/saibcm-modules
git -C sources/saibcm-modules checkout a1a9879e6aded405bd9f03462926d4223e33de8b
git -C sources/saibcm-modules apply ../../linux7-module-api.patch
python3 probe_linux7.py \
  /nix/store/ab1lk2mn87zinp5z9czj222p7sqydg11-linux-7.3.4-dev/lib/modules/7.3.4/build \
  --cc /nix/store/j1938gsbvbhkyz4kbmvjk56r479mkw3s-gcc-wrapper-16.2.0/bin/gcc \
  --pahole /nix/store/68w27bw25kig3fs47k0f4qswykldyid6-pahole-1.32/bin/pahole
```

The explicit compiler/kernel/pahole paths were already installed locally;
no global package installation or flake changes were made for this probe.
These paths must be replaced with matching installed derivations elsewhere.

## Critical ABI finding and next implementation step

OpenBCM 6.5.27 `include/kcom.h` declares **KCOM_VERSION 13**; the fetched 6.5.35
module tree declares **24**. Therefore these releases cannot be treated as
interchangeable kernel/userspace components. Renaming that version constant
alone does not establish compatibility. The stock 6.4.8 binary is also not
assumed compatible with either newer set of modules.

Preferred next implementation path:

1. Use full OpenBCM 6.5.27 source for matching userspace and kernel interfaces.
2. Backport kernel API fixes from the newer GPL drivers while preserving the
   6.5.27 ioctl/KNET contracts, or obtain a matching newer userspace SDK.
3. Cross-build for the T2080/e6500 big-endian target. Check 32-bit userspace
   on 64-bit PowerPC kernel compatibility, DMA addressing, cache coherency,
   PCIe interrupts, MMIO and packet byte swapping.
4. Reuse the captured board port/SerDes configuration, then validate attach,
   PHY links, isolated two-port VLAN forwarding, counters and CPU packet I/O.

Boot tests should initially use RAM/netboot or an alternate SD image. A clean
compile is necessary but does not prove DMA safety or ASIC operation.
