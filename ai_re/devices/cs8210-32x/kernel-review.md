# kernel review and Linux 7.x feasibility

Reviewed 2026-10-17. Target examined: upstream Linux **7.2.11**, downloaded from
kernel.org. Reference: user's [dmc12-nixos](https://codeberg.org/dma/dmc12-nixos),
commit `5d02d1b3de04082aea3960cd74db0c24f5b3ed32` (2026-07-25).

**Assessment:** a Linux 7.x serial/initramfs shell is a realistic initial port.
The dmc12 approach is directly useful for packaging, configuration and device
tree fixes. SSD boot looks plausible with upstream drivers. Management Ethernet,
board management and Innovium switching are additional milestones with more
uncertainty. The subsequent [offline follow-up](offline-research.md) found public
v3 HAL source, recovered board controls, and built a 7.2.11 candidate that reaches
a shell on QEMU virt. It has **not** been booted on CS8210 hardware.

## recovered artifacts

All paths below are relative to this project:

- ordinary DTB: `analysis/extracted/root/celestica_cs8210-r0.dtb` (48,463 bytes).
- 21-inch/12V variant: `analysis/extracted/root/celestica_cs8210-r0_12v_21.dtb`
  (47,587 bytes). These also appear under the extracted `root/image/os/`.
- decompiled ordinary DTS: `analysis/celestica_cs8210-r0.dts`.
- decompiled variant: `analysis/kernel-review/celestica_cs8210-r0_12v_21.dts`.
- recovered complete kernel config: `analysis/kernel-review/vendor.config`.
- decompressed ARM64 Image: `analysis/kernel-review/vendor-Image`.
- image header, CRC checks and version: `analysis/kernel-review/vendor-kernel.json`.
- module metadata and DWARF/symbol inventory: `module-info.txt` and
  `module-symbols.json` in `analysis/kernel-review/`.
- saved factory boot logs: `analysis/kernel-review/vendor-dmesg*.txt`, recovered
  from `/var/log/dmesg` and `/var/log/dmesg.0` inside the compressed SSD image.
- reference trees: `analysis/kernel-review/linux-7.2.11/` and `dmc12-nixos/`.

These analysis artifacts are ignored by git. The partition image temporarily used
to recover the boot logs was deleted. No source-SSD access was needed this turn.

## how standard is the existing kernel?

The uImage is an ordinary legacy U-Boot header containing a gzip-compressed ARM64
Linux Image. Both header and payload CRCs validate. Decompressed size is
17,637,888 bytes. Load and entry address are `0x04080000`.

Embedded banner identifies **4.9.124+**, built 2021-04-02 with GCC 6.3.0 and an
ONIE crosstool toolchain. Its embedded IKCONFIG was successfully decompressed
(117,940 bytes). It uses SMP, PREEMPT, 4 KiB pages, 48-bit virtual addresses,
modules, initramfs support and ext4 support. This is recognizable Linux with a
substantial vendor board-support layer, not a different kernel architecture.

Vendor config enables `AL_HAL`, `NET_AL_ETH`, `PCI_INTERNAL_ALPINE`,
`PCI_EXTERNAL_ALPINE`, `AHCI_ALPINE`, `ALPINE_SERDES_AVG`, `ALPINE_SERDES_HSSP`,
`ALPINE_IOFIC`, Alpine EDAC and reset support. Most of those exact configuration
symbols are absent upstream, although some functionality has upstream replacements.
The scan is saved in `vendor-platform-symbols.json`. An exact-string compatible
audit is also saved; absence of a string is not proof that equivalent hardware
support is absent, and a string's presence is not proof of working board support.

The full corresponding vendor kernel source/patch set was not found in the SSD
inventory, so a percentage or patch-count comparison would be speculative.
The driver archive contains modules rather than their C sources.

The stored boot log provides stronger evidence than the earlier diagnostic YAML:

- MIDR `0x410fd083` identifies Cortex-A72 r0p3; all 16 CPUs were brought up.
- Firmware supplied standard PSCI 0.2; CPUs entered Linux at EL2.
- GICv3 redistributors and the architectural timer were active.
- About 7.75 GiB of RAM was described to Linux, versus the packaged DTB's stub
  memory entries. Preserve firmware memory fixups and reservations.
- Internal PCI hosts SATA `1c36:0031` and Ethernet `1c36:0001` devices.
- External port 0 exposes Innovium `1d98:1b58`. Ports 1 and 4 were also active.

These are historical logs stored on the SSD, not a new observation of the live
chassis; a cloned factory image could contain another unit's logs.

## existing upstream support

Upstream [Alpine v3 DTS](https://github.com/torvalds/linux/blob/v7.2/arch/arm64/boot/dts/amazon/alpine-v3.dtsi)
describes the same 16-core topology and the same GIC, UART, internal ECAM and MSI
register addresses as the recovered board tree. It is a much better starting
point than carrying the entire 4.9 vendor tree forward.

| subsystem | evidence and implication |
| --- | --- |
| CPUs / SMP / timer | Standard ARM64, PSCI, GICv3 and architectural timer; favorable for early boot. |
| console | `ns16550a`, `0xfd883000`, SPI 17, register shift 2, I/O width 4; ordinary 8250 support. Stock UART clock is 500 MHz. |
| internal PCI | Upstream v3 uses `pci-host-ecam-generic` at `0xfbd00000`, matching the stock bridge's ECAM/ranges/legacy SATA interrupt map. Stock compatible must change. |
| SATA | Upstream `drivers/ata/ahci.c` explicitly matches `1c36:0031` with `board_ahci_al`, disabling PMP/MSI. Legacy interrupts are available in the upstream DT. Real DMA/SerDes behavior still needs testing. |
| external PCIe | Upstream `PCIE_AL` supports `amazon,al-alpine-v3-pcie`; modern binding requires config/controller/dbi resources. Stock nodes cannot be used unchanged. |
| MSI | Upstream Alpine MSI driver recognizes the stock fallback `al,alpine-msix`. Preserve correct SPI ranges and the interrupt-controller property. |
| I2C / basic sensors | DesignWare I2C, PCA954x muxes, AT24 EEPROMs and several standard sensors have upstream drivers; rebuild the board-specific bus topology. |
| management Ethernet | No upstream replacement found for the integrated `NET_AL_ETH` driver in the inspected tree. DMC12's standalone driver is a source starting point, with v3 gaps below. |
| fans / CPLDs / FPGA / PHY | Celestica modules and configurations need review and porting or suitable userspace access. Some individual regulator/sensor drivers have upstream equivalents. |
| switch ASIC | No upstream CS8210/Innovium switch implementation found. SDK depends on `ipd.ko` and userspace libraries; ordinary Linux boot does not establish switching support. |

The stock DTB has vendor SerDes, coherency, IOFIC and peripheral nodes that mainline
does not bind. That need not prevent an initramfs shell, but hardware initialized
by firmware versus by the old kernel must be distinguished before enabling DMA.

`THERMAL_MMIO` exists upstream, but the binding has changed: upstream recognizes
`amazon,al-thermal`, not the vendor `thermal-mmio` node with multiple sensor cells.
Do not assume an unchanged config symbol means unchanged device-tree behavior.

## what transfers from dmc12-nixos

The reviewed [kernel.nix](https://codeberg.org/dma/dmc12-nixos/src/commit/5d02d1b3de04082aea3960cd74db0c24f5b3ed32/kernel.nix)
uses the latest nixpkgs kernel, adds a board DTS, builds Alpine PCI/MSI/AHCI/ext4
support in, and builds a standalone `al_eth` module. It does not replace the
whole kernel with a vendor fork. This is the preferred structural approach here.

Useful details in its board DTS and boot-menu builder:

1. Add `interrupt-controller; #interrupt-cells = <1>;` to upstream `&msix`.
   Linux 7.2.11 `of_irq_init()` still skips nodes lacking `interrupt-controller`,
   and the upstream v3 MSI node still lacks it. The stock CS8210 DT already has
   `interrupt-controller`, so preserve that behavior when rebuilding the DTS.
2. Declare a real I2C input clock. DMC12 needed 500 MHz rather than a timer clock;
   CS8210's stock `sbclk` and UART clock are also 500 MHz. Its requested I2C bus
   speed is 400 kHz rather than DMC12's 100 kHz; don't copy timing blindly.
3. Describe external PCIe using `config`, `controller`, `dbi` register regions.
   For port 0, CS8210's stock addresses map to the same layout as the upstream
   v3 binding example: `0xfb600000`, `0xfd800000`, `0xfd810000`.
4. Wrap gzip ARM64 Image as a legacy uImage, and wrap an external initramfs as a
   ramdisk with compression type `none` so Linux decompresses it. This matches
   the existing CS8210 format and avoids requiring `booti`. **Follow-up correction:**
   derive the load address from the new Image header. Stock 4.9 uses a `0x80000`
   offset and loads at `0x04080000`; our 7.2.11 header has offset zero and the
   candidate loads at `0x04000000` to preserve required 2-MiB-base alignment.

Do not transplant the v2 DTS wholesale. CS8210 is v3: 16 Cortex-A72 cores,
different GIC/topology, internal ECAM `0xfbd00000` rather than v2 `0xfbc00000`,
and different board devices and MSI allocations.

**Concrete Ethernet gap:** DMC12's HAL defines only revisions 1 (v1), 2 (v2 basic)
and 3 (v2 advanced). Its integrated-device probe uses the PCI revision directly.
The CS8210 boot log instead reports `eth rev_id 2 (orig_rev_id 4) dev_id 1` for
both integrated NICs. The stock kernel also contains `al_hal_eth_mac_v3.c` and
AVG SerDes support not present in that DMC12 driver. The compatibility path is a
promising porting clue, not justification to relabel revision 4 as 2 blindly.
The [follow-up](offline-research.md) recovered that exact compatibility branch
from public HAL source, plus MAC v3/v4 and AVG SerDes implementation references.
It also identified the vendor PCI driver's explicit DMA/coherency programming.

DMC12's fan-controller addresses/registers and boot partitions are board-specific.
Its `0x04078000` DTB staging address is only 32 KiB below the kernel load address;
the full CS8210 DTBs exceed 32 KiB. Use a separate verified non-overlapping
layout rather than copying those staging addresses or depending on relocation.

## bootloader evidence

The SSD's bundled update image contains U-Boot
`2018.05-alpine_db-v3.65.1-HAL`, ONIE `2017.11.011`, built 2021-04-02.
Embedded defaults load a DTB from SSD partition 3 at `0x10000000`, load uImage
at `diagos_start=0x30000000`, and invoke `bootm`. Both `diag_bootcmd` and
`diagos_bootcmd` exist in that bundled default environment, resolving the README
spelling discrepancy. The bundled default `dtb_file=devicetree.dtb` differs from
the filenames found on disk, so actual saved variables must still be captured.

The binary contains a `booti` string, but that alone does not establish that the
installed U-Boot exposes the command. This is an update image, not a dump of
the current onboard flash. No bootloader or firmware update is needed merely
to attempt a RAM-loaded Linux test.

## difficulty and staged implementation

These are engineering estimates, conditional on access to a working serial
console, intact firmware initialization, and a repeatable RAM boot path:

| milestone | estimate / main unknown |
| --- | --- |
| Build 7.2 plus minimal board DTS and initramfs | Hours of implementation; compilation alone proves little. |
| Reach a serial shell with CPU/RAM/SMP | A few focused bring-up sessions, plausibly 1–3 days; firmware handoff, memory fixups and timer/GIC behavior could extend this. |
| SSD-root Linux | Additional sessions if upstream PCI/AHCI behaves; allow days to weeks if DMA coherency, SerDes or firmware dependencies emerge. |
| Management Ethernet and dependable board management | A real driver port; weeks are plausible, especially without the exact v3 source. |
| Full 32x400G switch operation | Separate, unbounded work until SDK/kernel-module sources and interface compatibility are established; potentially weeks to months or source-blocked. |

Suggested order:

1. Capture live serial boot, U-Boot `printenv`/`bdinfo`, and the runtime DT passed
   to the vendor kernel. Record clocks, RAM banks, secure reservations, MSI
   allocations, active PCI ports and boot image load limits.
2. Reuse DMC12's Nix structure with an Alpine-v3-derived CS8210 board DTS. Start
   with CPUs, PSCI, timer, GIC, UART and a tiny initramfs. Include the MSI fix when
   enabling PCI. Keep 4 KiB pages and built-in serial/initramfs support.
3. RAM-load the separately named test image and DTB, leaving the factory boot
   files and saved boot selection intact. Use `bootm` packaging first. Verify
   image/initrd/DTB staging and decompressed regions cannot overlap.
4. Bring up internal PCI, read-only SSD access and then a separate rootfs. Check
   SMP, interrupts, DMA and storage before treating this as a usable system.
5. Adapt v3 Ethernet support and the real board's fan/thermal controls. Establish
   fan behavior before sustained load. Then tackle external PCIe and FPGA/ASIC
   modules, preserving userspace ioctl interfaces where needed.

The supplied board modules and Innovium `ipd.ko` all target `4.9.124+`; binary
modules cannot simply be loaded into 7.x. `ipd.ko` does retain DWARF, symbols and
compilation-unit names (`ipd_main.c`, `inno_enet.c`, `inno_debug.c`), which helps
interface reconstruction if source cannot be obtained. Userspace SDK libraries
may be reusable if their kernel interface is preserved; that has not been tested.

Additional upstream references:

- [PCIe v3 binding](https://github.com/torvalds/linux/blob/v7.2/Documentation/devicetree/bindings/pci/amazon,al-alpine-v3-pcie.yaml)
- [Alpine PCIe implementation](https://github.com/torvalds/linux/blob/v7.2/drivers/pci/controller/dwc/pcie-al.c)
- [AHCI driver](https://github.com/torvalds/linux/blob/v7.2/drivers/ata/ahci.c)
- [ARM64 boot requirements](https://docs.kernel.org/arch/arm64/booting.html)

Local source inspection used 7.2.11; links above use the corresponding major
release tag for readable references. No hardware test is implied by source matches.
