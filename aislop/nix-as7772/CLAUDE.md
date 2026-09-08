# AS7772-32X NixOS Port — Claude Code Handoff

This document captures everything discovered and built in the initial reverse-engineering
and porting session for the **Accton AS7772-32X** network switch. Pick up from here.

> **Maintenance rule:** Update this file whenever `flake.nix`, the DTS, or any
> hardware finding changes. Keep it accurate — it is the primary context source for
> future Claude Code sessions.

---

## Project Goal

Run NixOS on a bare-metal **Accton AS7772-32X** (PowerPC) network switch. The immediate
milestone is a working netboot: kernel boots, management ethernet comes up, switch is
reachable over SSH from the lab network. From there: platform drivers, fan control,
QSFP management, and eventually BCM56960 SDK integration.

**This is not a SONiC port.** SONiC is x86-only. The target is NixOS on PowerPC with
ONL-style platform management, using the Broadcom SDK from the diag partition where needed.

---

## Hardware Identity

| Field | Value |
|---|---|
| Model | Accton AS7772-32X |
| Edgecore P/N | 7772-32X-A-AC-F-G-HVDC |
| Serial | 777232X19300200 |
| Platform string | `powerpc-accton_as7772_32x-r0` |
| HW revision | R01A |
| Manufactured | 2019-07-30, Taiwan |
| ONIE version | 2015.02.00.30 (very old, update before production use) |

---

## CPU / SoC

- **Freescale (NXP) T2080** PowerPC SoC
- Architecture: e6500 cores, PowerPC ISA 2.06
- 4 cores × 2 hardware threads = **8 hardware threads**
- ABI used in diag image: 32-bit PPC with soft-float (mpc85xx)
- **NixOS target ABI: 64-bit PPC big-endian, ELFv1 — `powerpc64-unknown-linux-gnuabielfv1`**
- RAM: **1920 MB** visible to OS (128 MB reserved for MMIO/firmware)
- U-Boot: 2014.07, version string `1.7.0.12`, built 2018-02-06

The U-Boot boots ppc64 Linux kernels (confirmed — ONIE runs `ppc64` kernel 3.12). The
kernel `ARCH=` is still `powerpc` for both 32- and 64-bit PPC. The BCM SDK modules from
the diag partition will **not** load into a 64-bit kernel (different ABI), so SDK
integration requires a rebuild from source or staying on 32-bit for that milestone.
Cross compiler: `powerpc64-unknown-linux-gnuabielfv1-gcc`.

---

## Switch ASIC

- **Broadcom BCM56960_A0** (Tomahawk, first generation)
- PCI Device ID: `0xb960`
- Connected via **PCIe3** (`ffe270000`), mapped at `0xd8000000` (128 MB, 32-bit space)
- SDK version in diag partition: **6.5.9** (built 2019-05-24)
- Switching capacity: 3.2 Tbps / 6.4 Tbps full duplex
- 32× QSFP28 ports (100G each); breakout: 64×50G, 128×25G, 128×10G
- **No hardware MACsec** — BCM56960 does not support it. Software MACsec via the
  kernel `macsec` driver + T2080 SEC 5.2 CAAM offload is the path forward if needed.

---

## Management Ethernet

- **FMan DTSEC1** (Freescale Frame Manager)
- Connection type: SGMII, 1 Gbps
- PHY: **BCM5400** at MDIO address `0x1f`
- Linux interface name will be something like `fm1-mac0`, `fm1-dtsec1`, or `eth0`
  depending on kernel version and FMan driver naming. Probe `/sys/class/net/` at runtime.
- U-Boot env: `ethact=FM1@DTSEC1`, `ethaddr=1C:EA:0B:38:2B:D5`
- TFTP server pre-configured at `192.168.1.99`; switch default IP `192.168.1.10`

---

## Flash Layout (NOR, 128 MB total at `0x0fe8000000`)

| MTD | Offset | Size | Label | Notes |
|---|---|---|---|---|
| mtd0 | 0x00000000 | 128 KB | RCW | Reset Config Word — do not touch |
| mtd1 | 0x00020000 | 128 KB | hw-info | Hardware EEPROM shadow |
| mtd2 | 0x00040000 | 32 MB | onie | ONIE FIT image (kernel+initramfs+DTB) |
| mtd3 | 0x02040000 | 32 MB | diag | Diagnostics image — **mine this for files** |
| mtd4 | 0x04040000 | 62 MB | open | NOS partition (currently empty) |
| mtd5 | 0x07f00000 | 128 KB | Fman-FW | FMan microcode — required at boot |
| mtd6 | 0x07f20000 | 128 KB | uboot-env | U-Boot environment |
| mtd7 | 0x07f40000 | 768 KB | uboot | U-Boot bootloader |

**ONIE partition detail:**
- Format: FIT (Flattened Image Tree), not legacy uImage
- Created: 2018-02-06
- Component 0: Kernel (3.69 MB, gzip, PowerPC)
- Component 1: InitramFS (9.75 MB, cpio)
- Component 2: DTB (36.28 KB) — `accton_as7772_32x-r0.dtb` — **already extracted**

Extract commands (run in ONIE shell after `dd if=/dev/mtd2 of=/tmp/mtd2.bin`):
```bash
dumpimage -T flat_dt -p 0 -o kernel.gz    mtd2.bin
dumpimage -T flat_dt -p 1 -o initramfs.cpio mtd2.bin
dumpimage -T flat_dt -p 2 -o as7772.dtb   mtd2.bin
dtc -I dtb -O dts -o as7772.dts           as7772.dtb
```

---

## U-Boot Boot Sequence

```
bootcmd = run boot_diag; run check_boot_reason; run nos_bootcmd; run onie_bootcmd
```

- `nos_bootcmd = true` → no NOS installed, always falls through to ONIE
- To netboot without modifying flash, override at the U-Boot prompt:

```
setenv nos_bootcmd 'dhcp; tftp $loadaddr as7772-nixos-netboot.itb; bootm $loadaddr'
saveenv
boot
```

Or one-shot without saving:
```
dhcp
tftp 0x1000000 as7772-nixos-netboot.itb
bootm 0x1000000
```

U-Boot DHCP vendor class: `uboot_vendor:powerpc-accton_as7772_32x-r0`

**⚠ U-Boot `bootargs` env var overrides the DTS `chosen` node.** The factory default is:
```
root=/dev/ram rw console=ttyS0,115200 quiet
```
The `quiet` flag suppresses all kernel output. Always override before booting:
```
setenv bootargs 'console=ttyS0,115200 root=/dev/ram rw'
```

---

## I2C Bus Topology

**Bus 0** (`0x118000`) — System management
- `0x4c` — ADT7461 temperature sensor
- `0x50` — AT24C256 EEPROM (TlvInfo: serial, MAC, platform name)
- `0x68` — DS1672 RTC

**Bus 1** (`0x118100`) — Management SFP
- `0x77` — PCA9546 4-ch mux → ch0 → SFP EEPROM at `0x50`

**Bus 2** (`0x119000`) — Empty/reserved

**Bus 3** (`0x119100`) — 32× QSFP28 data ports
- `0x71` — PCA9548 (reserved, no active children)
- `0x72` — PCA9548 → ports **9–12** (ch0–3), **1–4** (ch4–7)
- `0x73` — PCA9548 → ports **6,5,8,7** (ch0–3), **13–16** (ch4–7)
- `0x74` — PCA9548 → ports **17–20** (ch0–3), **25–28** (ch4–7)
- `0x75` — PCA9548 → ports **29–32** (ch0–3), **21–24** (ch4–7)

**⚠ Port numbering is non-linear across mux channels.** The mapping above was
reconstructed from `i2c_map-T2080-i2c1.csv` on the diag partition and cross-checked
against the original DTS. All QSFP28 EEPROMs present at I2C address `0x50` on their
respective mux channel; compatible string `at24,24c02`.

---

## Diag Partition — Critical Files to Extract

These files live on `mtd3` (diag partition) under `/usr/local/`. They are the **ground
truth** for all hardware configuration. Extract them before doing anything else.

### BCM SDK / ASIC config
| File | Purpose |
|---|---|
| `bin/bcm.user` | BCM shell binary (SDK 6.5.9) |
| `sbin/linux-kernel-bde.ko` | BCM kernel driver |
| `sbin/linux-user-bde.ko` | BCM userspace driver |
| `bin/config.bcm` | Default ASIC config |
| `bin/config.bcm-100G` | 100G port config |
| `bin/config.bcm-25G` | 25G breakout config |
| `bin/rc.soc` | BCM startup script |
| `bin/rc.soc-100G` | 100G startup script |
| `bin/switch_sdk_init.sh` | Full SDK initialization sequence |

### Hardware maps (CSV)
| File | Purpose |
|---|---|
| `bin/i2c_map-T2080-i2c1.csv` | I2C bus 0 device map |
| `bin/i2c_map-T2080-i2c2.csv` | I2C bus 1 device map |
| `bin/i2c_map-T2080-i2c3.csv` | I2C bus 2 device map |
| `bin/i2c_map-T2080-i2c4.csv` | I2C bus 3 (QSFP mux) device map |
| `bin/port_map-100G.csv` | BCM port ↔ front-panel port mapping, 100G |
| `bin/port_map-25G.csv` | BCM port ↔ front-panel port mapping, 25G |
| `bin/port_map-MFG.csv` | Manufacturing test port map |
| `bin/vmon_func_list.csv` | Power rail / voltage monitor map |

### Platform utilities
| File | Purpose |
|---|---|
| `bin/cpldutil` | CPLD register read/write |
| `bin/cpldtst` | CPLD test tool |
| `bin/fanmod_*` | Fan control scripts (airflow detect, RPM) |
| `bin/portled-*` | Port LED control scripts |
| `bin/qsfp` | QSFP management tool |
| `bin/sfpeeprom` | SFP EEPROM reader |
| `bin/i2cutil` | I2C utility |

### LED firmware
| File | Purpose |
|---|---|
| `bin/ES7632BTx.hex` | LED controller firmware |
| `bin/LED_100G.hex` | LED config for 100G mode |
| `bin/LED_25G.hex` | LED config for 25G mode |

---

## CPLD

- Mapped at IFC CS3, 768 bytes register space
- Compatible string in original DTS: `fsl,t2080-cpld` (no mainline driver)
- Controls: fan speed/direction, port present signals, LED mux, system reset
- The `cpldutil` binary from diag partition is the reference; reverse its register
  accesses to build a proper kernel driver or platform driver
- In the patched DTS it's given `compatible = "accton,as7772-cpld", "simple-mfd"` to
  avoid binding failures while still exposing the address range

---

## FMan Microcode

The FMan requires microcode firmware loaded from flash before the ethernet driver
initializes. In the original ONIE/diag setup this comes from `mtd5` (Fman-FW partition
at `0x07f00000`). The Linux `fsl_fman` driver looks for this via the device tree
`fsl,firmware` property or as a firmware file. Either:

1. Keep it in NOR flash and reference it from DTS, or
2. Embed `fsl_fman_ucode_T2080_106_4_18.bin` (or similar) into the initramfs

The exact microcode binary needs to be extracted from `mtd5`. Without it, the FMan
ethernet driver will fail to initialize and you will have no management network.
**This is the most likely first-boot failure point.**

---

## Device Tree Summary

The full patched DTS lives at `dts/accton_as7772_32x.dts` and is copied into the
kernel tree during the `postPatch` phase of the kernel build. Key
structural notes:

- Root: `#address-cells = <2>`, `#size-cells = <2>` (36-bit physical address space)
- SoC base: `0xffe000000` (uses `ranges` to flatten to single-cell offsets internally)
- PCIe windows use 36-bit `<high low>` addressing
- MPIC interrupt controller at `0xffe040000` — phandle `0x01`
- FMan at `0xffe400000` with internal range flattening
- BMan portals: `0xff4000000` (18 portals)
- QMan portals: `0xff6000000` (18 portals)
- `chosen` node sets console to `ttyS0` @ 115200, NFS root

**Changes made from the extracted ONIE DTS for kernel 6.1 compatibility:**
1. Added `chosen` node with `bootargs` and `stdout-path`
2. Changed `memory` node to `memory@0` with explicit `reg` (bootloader still fills in size)
3. Added `fsl,fman-cell-index` to the DTSEC1 ethernet node
4. Added `ptimer-handle` to DTSEC1 ethernet node
5. Changed CPLD compatible to `"accton,as7772-cpld", "simple-mfd"` to avoid binding failure
6. Completed all QSFP28 I2C mux child node entries (were incomplete in original)

---

## NixOS Flake — `flake.nix`

Located at the root of this repo. Builds three derivations:

### `packages.x86_64-linux.kernel`

Cross-compiles Linux 6.1 LTS for PowerPC e6500 from an x86_64 host.

- Base defconfig: `corenet64_smp_defconfig`
- Cross-system triple: `powerpc64-unknown-linux-gnuabielfv1` (ELFv1, big-endian)
- Board DTS copied from `dts/accton_as7772_32x.dts` into the kernel tree via `postPatch`
- DTB registered in `arch/powerpc/boot/dts/Makefile` via `postPatch`
- Kconfig overrides applied via `structuredExtraConfig` in `as7772KernelPatches`
- Outputs: `boot/uImage`, `boot/accton_as7772_32x-r0.dtb`, `boot/System.map`,
  `boot/kernel.config`, `lib/modules/<version>/`

### `packages.x86_64-linux.initramfs`

Minimal PowerPC initramfs containing:
- BusyBox for shell and basic tools
- `/init` script that: loads FMan modules, detects management interface, runs udhcpc,
  mounts NFS root, and pivot_roots into it
- udhcpc script for DHCP lease handling

### `packages.x86_64-linux.fitImage` (default)

Combines kernel + DTB + initramfs into a U-Boot FIT image (`as7772-nixos-netboot.itb`).
This is what you TFTP to the switch.

### `nixosModules.as7772-netboot`

Optional NixOS module for the server side. Configures:
- `atftpd` serving the FIT image
- NFS export at `/exports/as7772`
- `dhcpd4` with static lease for the switch MAC (`1c:ea:0b:38:2b:d5`)

Enable with:
```nix
{
  imports = [ as7772-flake.nixosModules.as7772-netboot ];
  as7772.netboot.enable = true;
}
```

### Dev shell

```bash
nix develop
# Sets ARCH=powerpc, CROSS_COMPILE=powerpc64-unknown-linux-gnuabielfv1-
# Has: bison, flex, perl, bc, openssl, elfutils, mkimage, dtc
```

---

## Build Commands

```bash
# Fix the kernel src hash first (see above), then:

# Build everything
nix build .#fitImage

# Build just the kernel
nix build .#kernel

# Build just the initramfs
nix build .#initramfs

# Copy to TFTP root
cp result/as7772-nixos-netboot.itb /var/lib/tftpboot/

# Inspect the FIT image contents
mkimage -l result/as7772-nixos-netboot.itb
```

---

## Lab Network Setup

```
Switch management port: 192.168.1.10 (static in U-Boot env)
TFTP/NFS server:        192.168.1.99
Switch MAC:             1c:ea:0b:38:2b:d5
Console:                ttyS0, 115200 8N1 (front USB console port)
DHCP vendor class:      uboot_vendor:powerpc-accton_as7772_32x-r0
```

Serial console access from U-Boot through Linux uses the same UART. No console
multiplexing is needed.

---

## Likely First-Boot Failure Points (in order of probability)

### 0. FIT image embeds uImage instead of zImage — **FIXED, was root cause of no console output**
**Symptom:** Complete silence after U-Boot `Loading Kernel Image ... OK`. Zero kernel output.
**Root cause:** `uImage` = 64-byte U-Boot legacy header + `zImage` payload. When embedded as
the `data` field in a FIT image subnode, U-Boot copies it verbatim to the `load` address
and jumps to `entry`. Both were `0x00000000`, so U-Boot jumped to the 64-byte header and
the CPU executed the magic bytes `0x27 0x05 0x19 0x56` as PPC instructions — instant chaos
before a single kernel instruction ran. **Fixed:** FIT now embeds `zImage` (no header) directly.

### 1. FMan microcode missing

**Symptom:** `fsl_fman` driver fails with `firmware load failed` or similar. No `eth0`.
**Fix:** Extract `mtd5` from the switch, find the `.bin` microcode file, embed in initramfs
at `/lib/firmware/` or reference from DTS. The NXP T2080 SDK page lists the microcode
as `fsl_fman_ucode_T2080_106_4_18.bin` — confirm the exact version from the diag image.

### 2. FMan driver API mismatch
**Symptom:** Kernel panics or oopses in `fsl_fman_port.c` or `dpaa_eth.c`.
**Fix:** Check NXP's downstream kernel tree (`github.com/nxp-qoriq/linux`, branch
`linux-5.15-rt` or `linux-6.1`) for T2080 FMan patches that haven't landed in mainline.
The `fsl,fman-memac` vs `fsl,fman-mac` compatible string may need adjustment.

### 3. Memory size not passed correctly
**Symptom:** Kernel sees 0 MB or wrong RAM size, crashes early.
**Fix:** The `memory@0` node has `reg = <0 0 0 0>` intentionally — U-Boot is supposed to
fill this in. If your U-Boot version doesn't update the FDT, add explicit size:
`reg = <0x00 0x00000000 0x00 0x78000000>` (1920 MB = 0x78000000).

### 4. Management interface name not found by init script
**Symptom:** `udhcpc` fails, "No interface found".
**Fix:** The `/init` script probes `eth0`, `fm1-mac0`, `fm1-dtsec1`, `management` in order,
then falls back to the first non-loopback interface. Add logging: `ls /sys/class/net/`
early in init. May need to adjust probing order.

### 5. NFS mount fails
**Symptom:** Init falls through to emergency shell.
**Fix:** Check server-side NFS export options. U-Boot might not have set IP via DHCP
before the kernel gets control. Add `ip=dhcp` to kernel cmdline in the `chosen` node if
not already present, or add `ip=192.168.1.10:192.168.1.99:192.168.1.1:255.255.255.0::eth0:off`
for static configuration during bring-up.

---

## Reference Platforms

These are the closest existing open-source platforms to use as templates:

| Platform | Where | Relevance |
|---|---|---|
| `accton_as7710_32x` | `machine/accton/` in ONIE repo | Same T2080 + BCM56960, best ONIE reference |
| `powerpc-accton-as7710-32x` | ONL repo | Closest ONL platform driver |
| `powerpc-accton-as5710-54x` | ONL repo | Good driver template, different ASIC |
| `t208xrdb` | mainline kernel `arch/powerpc/` | T2080 reference board, kernel config base |

ONIE repo: https://github.com/opencomputeproject/onie
ONL repo: https://github.com/opencomputeproject/OpenNetworkLinux
ONL porting guide: `$ONL/docs/PortingGuide.md`
NXP downstream kernel: https://github.com/nxp-qoriq/linux

---

## BCM SDK Notes

The BCM SDK in the diag partition is a binary blob (SDK 6.5.9). It is **not** open
source. The kernel modules (`linux-kernel-bde.ko`, `linux-user-bde.ko`) are pre-built
for the original kernel version and will not load into a 6.1 kernel without recompilation
(which requires the SDK source, available only under NDA from Broadcom).

**For the netboot milestone**, BCM SDK integration is not needed. The switch ASIC will
simply not initialize; the BCM56960 will be visible as a PCI device but inert. The
management port works independently of the switching ASIC.

**Long-term options for switching:**
1. Obtain BCM SDK source under Broadcom NDA and rebuild modules for your kernel
2. Use the `OF-DPA` / OpenNSL approach if Broadcom releases a version supporting BCM56960
3. Use `linux-kernel-bde.ko` from the diag partition as-is with a matching kernel build
   (match the original kernel version 3.x from the ONIE image, then update gradually)

The `config.bcm` and port map CSVs from the diag partition are needed regardless of
approach — they encode the physical lane assignments that are board-specific.

---

## Immediate Next Steps for Claude Code

**Step 1 — Extract FMan microcode from the switch**
```
# Boot into ONIE, then:
dd if=/dev/mtd5 of=/tmp/fman-fw.bin bs=1k
tftp -p -r fman-fw.bin 192.168.1.99   # or use netcat
# Find the actual .bin microcode file embedded in the partition
# Add it to the initramfs at /lib/firmware/
```

**Step 2 — Try the build**
```bash
nix build .#fitImage 2>&1 | tee build.log
```

Watch for cross-compilation errors in the FMan driver sources. The most likely issue
is a changed API between the original kernel and 6.1 in `drivers/net/ethernet/freescale/fman/`.

**Step 3 — First boot test**
```bash
# On server
cp result/as7772-nixos-netboot.itb /var/lib/tftpboot/
# Set up NFS export with a minimal rootfs (Debian PPC debootstrap or buildroot)
# On switch (U-Boot):
dhcp
tftp 0x1000000 as7772-nixos-netboot.itb
bootm 0x1000000
```

Watch serial console carefully. The FMan microcode and memory map issues will show up
in the first few seconds of boot.

**Step 4 — Platform driver**
Once netboot is stable, the next major piece is a platform driver that talks to the CPLD.
Base it on `drivers/platform/x86/` style or create `drivers/platform/powerpc/as7772.c`.
The CPLD controls fans (critical — without it they run at 100%), port present signals,
and LED state. Use `cpldutil` from the diag partition to reverse the register layout.

---

## File Structure of This Repo

```
.
├── CLAUDE.md          ← you are here; keep it updated as the project evolves
├── README.md          ← human-facing: build instructions, boot procedures, lab network
├── flake.nix          ← main build: kernel + initramfs + FIT image + NixOS module
├── dts/
│   └── accton_as7772_32x.dts  ← board DTS (patched from ONIE original for kernel 6.1)
└── (future)
    ├── platform/
    │   ├── cpld.c     ← CPLD kernel driver
    │   ├── fans.c     ← fan control
    │   └── qsfp.c     ← QSFP management
    ├── nixos/
    │   ├── configuration.nix   ← full NixOS config for the switch
    │   └── hardware.nix        ← hardware-specific NixOS module
    └── scripts/
        ├── extract-diag.sh     ← extract useful files from diag partition
        └── setup-netboot.sh    ← configure server-side TFTP/NFS/DHCP
```
