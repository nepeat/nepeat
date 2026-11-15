# upstream U-Boot on the DMC-12: RAM chainload (2026-11-15)

**Goal of this stage:** the factory U-Boot 2015.07 (`ALPINE_DB>`) loads upstream
U-Boot 2026.10 into RAM and jumps to it. The new U-Boot boots Linux the modern way:
raw arm64 `Image` + `booti`, extlinux via `bootflow scan`, from the SATA SSD's ext4.
Nothing is flashed. SPI NOR, the env and the SSD are only ever read.

**Status (2026-11-15, u01): RAM chainload works on hardware.** The factory `bootm`
entered at EL2 with MMU+caches off. Console at 500 MHz, `DRAM: 3 GiB (total 4 GiB)`,
relocated to 0xaff6a000. I2C through both muxes to the CPLD (0x01 = 0x88), internal
PCIe ECAM (4 SoC devices), AHCI + SSD, GPT read. `bootflow scan -l` finds none (the
factory SSD has no extlinux). `reset` → CPLD → stage2 in 2 s. Not run: `--go`,
`--boot`, booting Linux. Not in this build: the `dm` command (dropped from the harness
since), external PCIe (the USB bridge: no U-Boot driver for `al-alpine-v2-pcie`, not
needed). SCSI capacity reads one sector high (62533297 vs Linux 62533296; QEMU shows the
same +1), harmless for partition reads. Transcript:
`serial-chainload-20261115-042317.txt` (git-ignored).

## what exists

- `nix build path:./nix#uboot-dmc12-chainload` → `u-boot.uimage` (for `bootm`),
  `u-boot.bin` (for `go`/`booti`), `u-boot`, `.map`, `.dtb`, `.config`.
  Store path `/nix/store/7w61vis50vjz9p5vjm4vs4czbn0wsjba-…-2026.10`.
  `u-boot.bin` sha256 `66daa630…`, `u-boot.uimage` sha256 `7634a36c…`.
- Patches: shared series [ai_re/nix/packages/uboot/default.nix](../../nix/packages/uboot/default.nix):
  - 0014 `alpine-v2` (any Alpine V2 board): new target `TARGET_ALPINE_V2`, with no low-level init.
  - 0015 `dmc12`: CPLD sysreset driver.
  - 0016 `dmc12`: `-u-boot.dtsi` and `alpine_v2_dmc12_defconfig`.

  The T208x builds take only their own scopes, and their `.drvPath`s are unchanged.
  The board DTS is the Linux one, `nix/packages/linux-alpine/boards/dmc12/alpine-v2-dmc12.dts`,
  copied in at build time. A DTS edit there rebuilds U-Boot.
- Tests: `nix build path:./nix#uboot-alpine-v2-tests` (no board needed, both pass).
  - **static:** AArch64 ELF linked at 0x08000000, Linux Image header, uImage header,
    RAM footprint, no `smc`/`hvc` instruction anywhere. The config is checked to have
    no SPI flash, env storage, network, MMC, EFI or PSCI reset, and the sp805 is not
    auto-started. The DT is checked for the memory banks, firmware ranges, CPLD and
    sp805 nodes, and the 500 MHz UART bound before relocation.
  - **qemu-virt:** the same patches and board code on `virt,highmem=off,virtualization=on`
    (Cortex-A57, EL2), with a test DT ([qemu-virt.dts](../../nix/packages/uboot/alpine-v2/qemu-virt.dts)).
    1. Entered like the vendor `bootm`: MMU off, x0 = DTB.
    2. Relocates below a reserved top range.
    3. `bootflow scan -lb` goes PCI ECAM → AHCI → GPT → ext4 → extlinux, then `booti`s a
       probe "kernel". The probe sees EL2, a valid FDT, and the reserved ranges in its
       memreserve map, even though its DTB had no `/reserved-memory`.
    4. From that U-Boot's prompt (MMU and caches on), `go` to a second copy reaches a
       prompt that reports the MMU-on entry.

    QEMU doesn't model caches, so this proves the code path, not the cache maintenance.

## design (why it is shaped like this)

- **Mainline U-Boot has no Alpine support** (checked: the 2026.10 tree has no
  `mach-alpine`, no board, no driver; "alpine" only in mach-types and docs). Annapurna's
  `alpine_db` U-Boot 2015.07 is GPL, but a quick search found no published source.
  Places to look: QNAP GPL drops for AL-314/AL-324 NAS (TS-x31X/x32X), Netgear R9000 /
  ReadyNAS, Synology DS416, or a request to Edgecore. Nothing here depends on it.
- **No low-level init.** al-boot stage2/3 has trained DRAM, set up SerDes and the PCIe
  unit adapters, and installed the PSCI v0.1 monitor. Secondaries stay with the firmware,
  and Linux starts them with PSCI `cpu_on` as before. U-Boot itself makes no SMC.
- **Entry.** `bootm` of a *kernel-type* uImage (`-A arm64 -O linux -T kernel`, load =
  entry = 0x08000000) makes the factory loader clean up as it does for Linux: caches and
  MMU off, entry at EL2 with x0 = its DTB. `-T standalone` would not, and `go` doesn't
  either. For `go`, `save_boot_params` (patch 0014) cleans the D-cache by set/way and
  turns MMU/caches off before start.S touches memory. The banner line
  `Entry: EL<n>, MMU+caches were on (go)/off (bootm), x0=…` says which path ran. It also
  answers "**at which EL does the factory U-Boot run**". We expect EL2: the vendor kernel
  logs "All CPU(s) started at EL2", and the 2015 armv8 bootm only drops EL3→EL2, never
  further. The string `got EL3 abort!` sits in al-boot stage3, so EL3 is probably the
  firmware's.
- **Position independent**, linked at 0x08000000 (no fixups when loaded there). It also
  carries a Linux Image header, so a later build can be `booti`'d from this one.
- **Memory.** Banks come from `/memory` in `alpine-v2-dmc12-u-boot.dtsi`: 0–3 GiB and
  8–9 GiB, as the vendor's DT has them. The MMU map is DRAM = Normal plus every hole
  below 4 GiB = Device. The firmware ranges come from the vendor loader's `/memreserve/`
  list (0x0 +1 MiB, 0x02000000 +1 MiB, 0xb0000000 +256 MiB; contents unknown). They are
  `/reserved-memory` in the U-Boot DT: lmb won't load over them, U-Boot relocates below
  0xb0000000, and `ft_board_setup()` adds them to the reserve map of **whatever DTB
  Linux gets**. The kernel-tree DTS has none of them. The vendor's other two memreserves
  are only its own DTB and initrd.
- **Reset** (`reset`, panic): sysreset tries the board CPLD first (i2c-4 / 0x68: 0x29←1,
  then 0x28←1, as the vendor kernel does). If the board is still alive after 1 s, it
  falls through to the SoC sp805 (`wdt-reboot`, load 0). There's no PSCI SYSTEM_RESET
  (the firmware has none). The sp805 is never started otherwise, so Linux doesn't
  inherit a running watchdog. Untested on hardware.
- **No network:** al_eth has no U-Boot driver. Boot media is SATA only.

## load addresses

| range | owner |
| --- | --- |
| 0x0 +1 MiB, 0x02000000 +1 MiB | firmware (reserved) |
| ~0x03B00000–0x04000000 | factory U-Boot (stack 0x03B04CD0, `fdtaddr` 0x03B06010, relocaddr 0x03F0A000) |
| 0x04078000 +28 KiB | factory DTB copy (`loadaddr_dt`) |
| **0x08000000–0x080CE5F0** | **this image** (code + BSS + 512 KiB early stack; dead after relocation) |
| 0x0f000000 / 0x0f100000 / 0x0f200000 / 0x0f300000 | `fdt_addr_r` / `fdtoverlay_addr_r` / `scriptaddr` / `pxefile_addr_r` |
| 0x0fd00000 | factory initrd staging (not used by us) |
| 0x10000000 | uImage TFTP staging (factory side), then `kernel_addr_r` |
| 0x20000000 +64 MiB | `kernel_comp_addr_r` (Image.gz) |
| 0x30000000 | `ramdisk_addr_r` |
| just below 0xb0000000 | relocated U-Boot, malloc, its FDT; OS DTB/initrd copies go under it |
| 0xb0000000 +256 MiB | firmware (reserved) |

## first hardware test (main agent runs it; RAM only)

Artefacts are staged in `/srv/tftpboot/dmc12-uboot/` (TFTP served, sha256 checked).
The box must be at an idle `ALPINE_DB>` (`./powercycle_to_uboot.py`). Then either the
harness, which writes `serial-chainload-<date>.txt` and exits 0 only on PASS:

```sh
./chainload_uboot.py nixos-build/result-uboot-dmc12-chainload           # bootm path, read-only checks
./chainload_uboot.py nixos-build/result-uboot-dmc12-chainload --go      # MMU-on path
./chainload_uboot.py … --reset     # then also: reset via the CPLD, wait for stage2
./chainload_uboot.py … --boot      # then also: bootflow scan -lb (needs extlinux on sda1)
```

or by hand:

```
ALPINE_DB> setenv autoload no; dhcp
ALPINE_DB> setenv serverip 10.60.2.208
ALPINE_DB> tftpboot 0x10000000 dmc12-uboot/u-boot.uimage
ALPINE_DB> iminfo 0x10000000
ALPINE_DB> bootm 0x10000000 - ${fdtaddr}
#  "go" variant:  tftpboot 0x08000000 dmc12-uboot/u-boot.bin; go 0x08000000
```

The `- ${fdtaddr}` (the factory's live DTB) is only there because the 2015 arm64 bootm has
no ATAGS fallback and stops without an FDT (from the 2015 source, not tried). The new
U-Boot ignores it and uses its own DT. Expect `U-Boot 2026.10-dmc12-chainload`, the `Entry:` line,
`DRAM: 4 GiB`, a 3 s countdown (any key), then `=>`. Useful read-only checks: `bdinfo`
(relocaddr < 0xb0000000), `i2c dev 4; i2c md 0x68 0x01 1` (CPLD version 0x88),
`pci` (1c36:0031 AHCI), `scsi scan`, `part list scsi 0`, `ls scsi 0:1 /boot`,
`bootflow scan -l`.

Interrupting the factory autoboot skips its preamble, so the fans stay at full speed
(safe, loud). `run clear_wdt_flag; run fan_speed_70` at the factory prompt (both are
I2C writes to the CPLD) quiets them first, if wanted.

## booting Linux from it

- Standard boot: `bootflow scan -lb` (the default `bootcmd`) finds
  `/boot/extlinux/extlinux.conf` or `/extlinux/extlinux.conf` on any SCSI partition. It
  loads `KERNEL` (a raw `Image`), `INITRD` and `FDT`, or `FDTDIR` +
  `fdtfile=amazon/alpine-v2-dmc12.dtb`. With no FDT line it uses its own DT, which is the
  Linux board DTS plus the U-Boot additions. It fixes up `/memory` and the reserve map,
  then `booti`s at EL2.
- By hand: `ext4load scsi 0:1 ${kernel_addr_r} /boot/Image; ext4load scsi 0:1
  ${ramdisk_addr_r} /boot/initrd; setenv rd_size ${filesize}; ext4load scsi 0:1
  ${fdt_addr_r} /boot/alpine-v2-dmc12.dtb; setenv bootargs console=ttyS0,115200
  earlycon=uart,mmio32,0xfd883000 …; booti ${kernel_addr_r} ${ramdisk_addr_r}:${rd_size}
  ${fdt_addr_r}`.

## on-disk boot (proposed, build-checked only)

[nix/modules/boards/dmc12/boot-chainload.nix](../../nix/modules/boards/dmc12/boot-chainload.nix),
an alternative to `uboot-menu`. The chain:

1. **Factory U-Boot**, with the reimage.md env change (`bootcmd=run nixos_boot; run
   dmc12_bootdisk`, `nixos_boot` sources `/boot.scr` from sda1). The env change is the
   same for both loaders: only `/boot.scr` differs.
2. **`/boot.scr`** (2015 hush): `run clear_wdt_flag; run fan_speed_70` (the factory
   preamble's fan/WDT writes), `scsi init`, then
   `ext4load scsi 0:1 0x10000000 /u-boot-dmc12.uimage && iminfo` →
   `bootm 0x10000000 - ${fdtaddr}`. This is the same entry as the tested TFTP path.
   If the load or checksum fails, it tries `/u-boot-dmc12.prev.uimage`, then returns, and
   the env falls through to `dmc12_bootdisk`.
   - The upstream U-Boot's script bootmeth would also find `/boot.scr` (after extlinux).
     A `kernel_comp_addr_r` guard (only in the upstream env) stops it from chainloading
     itself in a loop.
3. **Upstream U-Boot**: `bootflow scan -lb` → sda1 (MBR, bootable flag) →
   `/extlinux/extlinux.conf`, written by nixpkgs' own extlinux builder. That file has
   `LINUX`/`INITRD` as raw `Image` and initrd, `FDT …/amazon/alpine-v2-dmc12.dtb`, a 3 s
   menu and 10 generations. U-Boot then `booti`s it at EL2, with the memreserves copied in.

The installer (activation, and `dmc12BootBuilderNative <toplevel> -d <dir>` for
disk.nix) runs that builder. It then puts `u-boot-dmc12.uimage` and `boot.scr` on /boot,
atomically and only when they change, keeping the previous U-Boot as `.prev`.

- **Checked:** the module evaluates for the dmc12 system. It ships the hardware-tested
  uImage (sha256 `7634a36c…`). In a sandbox, staging a fake toplevel gives the expected
  /boot layout and extlinux.conf.
- **Not yet run:** the factory `ext4load` of a 368 KiB file from our ext4. It reads
  `/boot.scr` there already (first iteration). The real kernel through `booti` also
  hasn't run.
- **Recovery:** the factory countdown (5 s) still comes before `bootcmd`. From that
  prompt, `run dmc12_bootdisk` or the TFTP chainload above work. A U-Boot that hangs after
  `bootm` can't fall back by itself; power-cycle and interrupt.
- **To switch a system over:** `disk.nix` imports `./uboot-menu` unconditionally, and
  both modules force `installBootLoader`. Import `./boot-chainload.nix` there instead (or
  gate both behind an option). Its mkfs features stay: the factory loader still reads
  sda1. Nothing in `default.nix`, `disk.nix` or the platform module is changed yet. The
  platform comment "vendor U-Boot hands the kernel a legacy uImage" would then be stale.

## risks / unverified

- **Nothing has run on the SoC yet.** The DMC-12 console, PCI and I2C paths are only
  build-tested. QEMU covered the generic board code.
- Arch timer: U-Boot uses `CNTFRQ_EL0` as the firmware left it. The vendor DT also
  carries `clock-frequency = 66.66 MHz`. If CNTFRQ were 0, delays and the autoboot
  countdown would break. In that case: hang after `DRAM:`, or a countdown that never ends.
- `go` path: set/way clean on the A57 cluster is the standard method but unproven here.
  Prefer `bootm`.
- The DM serial re-inits UART0 from the DT clock: 500 MHz, divisor 271, 115313 baud
  (+0.1%). A wrong clock shows as garbage after the banner. The debug UART (no re-init)
  prints `<debug_uart>` first.
- PCI: U-Boot re-assigns BARs on the internal ECAM bus inside 0xfe000000 +16 MiB. Linux
  does this again anyway (`pci=realloc`). External PCIe (USB hubs) is not touched.
- 0x02000000 +1 MiB holds the al-boot trace buffer (`uboot-vendor-commands.md`). The
  other ranges' contents are unknown, and so is whether other Alpine units or
  al-boot versions use the same ones. They stay board-level (dmc12 dtsi) until then.
- Old kernels without Image-header flag bit 3 would be relocated by `booti` to DRAM base
  0x0, which is firmware. Every current kernel sets the bit.
- If the SSD holds a `boot.scr`, `bootflow scan -lb` runs it. The first iteration's
  script calls `bootmenu`, which isn't compiled in, so it fails without effect. u01's
  sda1 is not known to have one.
- The uC watchdog (10 s, "enabled") never fired in the factory U-Boot, which never
  services it either (disassembly). Watch for an unexpected reset while sitting at `=>`.
- **On-disk boot gaps** (`uboot-vendor-commands.md`, "What the factory boot path does"):
  - Linux al_eth needs the board params and MAC that the factory U-Boot writes only after
    a network command. The proposed `/boot.scr` runs none.
  - DN0-variant units need the preamble's SATA SerDes write.
