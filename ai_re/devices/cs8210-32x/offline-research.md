# offline follow-up — 2026-10-17

We now have a compiled Linux 7.2.11 serial/initramfs candidate, a compact board
DTB, and a successful **QEMU virt** shell test. Nothing has been booted on the
CS8210. No switch connection, source-SSD access, or firmware installation was
used for this work.

The strongest new source finding is a public Alpine v3 HAL. The strongest new
board findings are the CPLD watchdog/fan register behavior and an explicit
runtime board-variant discriminator. These reduce uncertainty substantially,
but do not establish working Ethernet, storage or switching on Linux 7.

## public source found

The [MikroTik GPL source mirror](https://github.com/tikoci/mikrotik-gpl/tree/c3e110db1d35886c96ee14e16fc5a06bcac59692/2025-03-19)
contains `linux-5.6.3.patch` (24,886,104 bytes), with Annapurna HAL version `2.6`.
It is not the exact Celestica source release. The patch's source timestamps are
January 2022, later than our April 2021 image.

- Repository commit: `c3e110db1d35886c96ee14e16fc5a06bcac59692`.
- Patch SHA-256: `fb56d0732153188fcd72e13224037ae92293c0cbb1741787eea432cabe55ef18`.
- Local patch: `analysis/research/mikrotik-linux-5.6.3.patch`.
- Reconstructed newly added files: `analysis/research/mikrotik-added/`.
- Extraction manifest: `analysis/research/mikrotik-added-manifest.json`.
- 1,395 added files reconstructed from complete addition hunks; this is a
  source-reference corpus, **not a patched/buildable full MikroTik kernel**.
  The GitHub recursive tree was truncated; the complete downloaded patch was
  inspected independently of that incomplete tree.

Relevant files under `drivers/soc/alpine/HAL/` include:

| Source | What it resolves |
| --- | --- |
| `drivers/eth/al_hal_eth_main.c` | Exact original-revision-4 / effective-revision-2 compatibility behavior |
| `drivers/eth/al_hal_eth_mac_v3.c`, `al_hal_eth_mac_v4.c` | Newer MAC implementations absent from the DMC12/v2 driver |
| AVG SerDes drivers and headers | v3 register/programming reference, instead of guessing from v2 |
| `include/eth/al_hal_eth.h` | Device ID 1 = standard Ethernet; revision 4 belongs to Alpine v3 |
| PCIe, unit-adapter, DDR and system-fabric files | Register definitions and initialization ordering |

`al_eth_adapter_handle_init()` at approximately line 1089 stores the original
revision, then maps standard device ID 1 / revision 4 to effective revision 2.
This matches the captured CS8210 boot log exactly. So the revision mismatch is
intentional compatibility behavior, not evidence that the CPU is Alpine v2.

This is a better HAL reference than the DMC12 driver alone. A modern Linux
network-driver wrapper, board-specific PHY/SerDes configuration, and actual
link/DMA testing are still required. The patch did not supply a complete
CS8210 `al_eth` Linux driver or its board support. Do not apply the entire
MikroTik patch to a mainline kernel; it includes extensive unrelated changes.
The HAL source inspected offers GPLv2 and alternative licensing terms in-file.

There is also a useful independent feasibility report: the developer in
[this MikroTik thread](https://forum.mikrotik.com/t/turning-ccr2116s-and-rds2216s-into-proxmox-ceph-and-or-truenas-hosts/269481)
reported management Ethernet on a similar Alpine v3 platform in May, followed
by successful Linux 6.12.74 boots later in the thread. This is a first-person
report about different hardware, not a CS8210 validation. No downloadable
source for that completed port was found in the thread or the author's public
repository list checked here.

## PCI/AHCI: a concrete missing initialization path

The recovered reference `drivers/pci/controller/pci-internal-alpine.c` has a
`BUS_NOTIFY_BIND_DRIVER` callback which:

1. Marks devices DMA-coherent through `arch_setup_dma_ops()`.
2. For bus 0 physical functions, sets bits 0 and 1 at PCI config offset `0x110`.
3. For slots below 6, mirrors setup to VF config offsets `0x130/0x150/0x170`.
4. Replaces the low 16 bits of config `0x220` with `0x03ff`.

The CS8210 historical log's `writing to VF config space` message matches this
source. This is strong evidence of a shared vendor initialization lineage,
though it does not prove every instruction is identical in the stock kernel.

Upstream `pci-host-ecam-generic` describes/enumerates the internal bus but does
not implement this vendor callback. Adding `dma-coherent` to a DTS describes
hardware behavior; it does not program these registers. Conversely, whether
firmware already leaves the needed state configured is still unknown. Capture
these config registers under stock firmware before designing a targeted
mainline fixup. This is an explicit dependency for SATA and Ethernet DMA,
not merely a device-ID question.

The vendor reference also uses full-width reads followed by software extraction
for byte/word config reads. Generic ECAM access width is another detail to check
against the actual adapter before calling the two implementations equivalent.
The serial candidate disables PCI completely, so it does not depend on these
unresolved details.

## ONIE recovery image decoded

`scripts/unpack_onie.py` extracts the self-extracting updater as data, checks its
embedded payload SHA-1, validates all four FIT CRC32 values, and inventories
1,400 cpio entries. Installer code is never executed. Only regular files are
written; links and special nodes remain inventory records.

Local output: `analysis/research/onie/`, with `inventory.json`, `installer/`,
`update/`, `fit/`, and `rootfs/`.

| Item | Finding |
| --- | --- |
| Build | ONIE `2017.11.011`, April 2 2021, `arm64-celestica_cs8210-r0` |
| FIT kernel | gzip, 7,028,738 bytes; stock load/entry `0x04080000` |
| FIT initramfs | 19,706,392 bytes; XZ-compressed cpio, FIT compression marked `none` |
| DTB 1 | 48,463 bytes, load `0x10000000`, default configuration `celestica_cs8210` |
| DTB 2 | 47,587 bytes, load `0x10400000`, configuration `celestica_cs8210_12v_21` |
| Environment configuration | `/dev/mtd1 0 0004000 00040000` in ONIE `fw_env.config` |
| Firmware package | U-Boot, both board DTBs, ONIE, system/fan/LED/MDIO FPGA/CPLD and ASC images |

The environment sizes above are retained verbatim; verify the installed flash
layout and tool interpretation before using them. They are not a flash backup.

ONIE's platform startup unpacks the board drivers and diagnostics, runs their
installer, and may initialize the SDK/Credo PHY depending on `boot_reason`.
Its network hook explicitly stops ONIE discovery. Consequently, the factory
ONIE image is a board-initializing environment, not a passive rescue shell.
The custom candidate uses only BusyBox and four runtime libraries from it,
with our own minimal `/init`; it contains none of that startup code.

ONIE install metadata says `image_partition_type=gpt`; the captured SSD actually
has an MBR partition table. Build metadata does not override observed media.

## board controls and hardware map

See [board-control.md](board-control.md) for the recovered mux paths and register
semantics. `analysis/research/i2c-map.json` inventories 75 I2C-compatible nodes
in the ordinary DT and 66 in the alternate DT, including muxes/controllers.
These counts describe packaged trees, not detected physical components.

Especially useful: stock startup reads system-CPLD register `0x09` bit 0 to
select the 21-inch/12-V configuration when the bit is **zero**. It reads the
upper nibble of register `0x01` for the board revision and MDIO driver choice.
This gives us a direct identification path once stock software is running.

## Innovium driver ABI recovered

The SDK's `ipd.ko` retains DWARF, including its ioctl layouts. Running
`scripts/extract_ipd_abi.py` produces `analysis/research/ipd-abi.json`, containing
16 named ABI/support typedefs, nested member types, byte offsets and sizes.
The dispatcher uses the command's low byte, with entries 1 through 26; the
captured switch-table targets are recorded too. This is not a complete mapping
from command IDs to operations yet.

Examples from the ARM64 build:

| Type | Bytes | Purpose suggested by fields |
| --- | ---: | --- |
| `inno_ioctl_hdr_t` | 8 | Version at 0, instance at 2 |
| `inno_ioctl_query_t` | 112 | BAR0, DMA pool, interrupt/writeback offsets, PCI identity |
| `inno_ioctl_ring_t` | 48 | Ring/writeback addresses, count, flags, indices |
| `inno_ioctl_rupt_wait_t` | 64 | Interrupt vector, timeout and status |
| `inno_ioctl_netdev_t` | 48 | System port, MAC address, flags, interface name |
| `inno_ioctl_netdev_info_t` | 76 | Interface ID, system port and operation union |
| `inno_ioctl_tx_send_t` | 64 | Buffer, ring, length, descriptors and flags |

This makes an ABI-preserving replacement/port more tractable. It does not
recover the complete DMA, interrupt, synchronization or packet-processing
implementation, and the SDK's userspace libraries remain binary components.
Keep the existing ARM64 userspace together initially; migrating the kernel and
migrating its old libc/Python environment are separate compatibility problems.

## concrete Linux 7 candidate

See [bringup/README.md](bringup/README.md) for files, rebuild steps and test scope.
The kernel is unmodified upstream 7.2.11 with a small config and embedded
initramfs. The board DTS includes the upstream Alpine v3 SoC description and
adds the UART clock, console, conservative placeholder memory, and disabled
PCI/MSI. No vendor kernel patches are included.

The significant boot-address correction: the stock Image header has
`text_offset=0x80000`, but the new Image header has `text_offset=0`. ARM64 requires
the Image at a 2-MiB-aligned base plus that offset. The candidate uImage therefore
uses **`0x04000000`**, not the stock/DMC12 `0x04080000`. See the
[ARM64 boot protocol](https://docs.kernel.org/arch/arm64/booting.html).
The build script derives this from the actual header rather than hardcoding
the old address. Live RAM reservations and the installed bootloader still need
checking before using this proposed layout.

## negative results and remaining blanks

Current recursive trees checked: ONIE (`ce480be7094deef7198ee4d50af8427ea89b93e3`),
SONiC buildimage (`408ffc98c3dc4e772ab055c2dbcd7ea2650ea5e7`), and Marvell's platform
repository (`ba78ac7561bd8e8afb55ce5791195310980170c6`). No exact CS8210/Briggs
platform appeared in those snapshots. GitHub repository-name search for CS8210
returned zero results. This is a bounded search, not proof no public source exists.
The Innovium platform fork inspected at
`krismarvell/sonic-platform-innovium@71fa68138294577843cf24207ce33a9e2407c044`
contains a different board's platform modules, not the needed Innovium SDK/IPD
source. Discovery responses are retained under `analysis/research/`.

| Remaining question | Best next evidence |
| --- | --- |
| Exact installed U-Boot/firmware and saved environment | Serial `version`, `printenv`, `bdinfo`, command help |
| RAM banks, secure reservations, firmware DT fixups | Live `/sys/firmware/fdt` or `/proc/device-tree`, stock dmesg, U-Boot memory report |
| Which board variant is physically installed | Chassis/board inspection plus CPLD identification values |
| Cooling across handoff and watchdog expiry behavior | Observe stock fan telemetry, documented control changes and timed behavior with power control available |
| SATA/USB/management-NIC DMA initialization | Stock PCI config snapshot and controlled mainline trials |
| v3 Ethernet port | Integrate selected HAL changes into a maintained wrapper, then test PHY/link/interrupt/DMA |
| Front-panel switching on mainline | Complete IPD ABI mapping/source recovery or port, retain SDK/Credo setup, test port modes |
| Recovery completeness | Separate backups of installed SPI flash, EEPROMs, current DT/env; update payloads are insufficient |

The next hardware session should first collect evidence under the working
firmware, then try the separately staged RAM image. It need not install or
replace the factory OS to answer whether mainline reaches a console.
