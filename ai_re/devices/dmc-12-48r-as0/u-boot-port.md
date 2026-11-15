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

**Ethernet (2026-11-15): built and offline-tested, not yet run on hardware.** The al_eth
driver (`dhcp`, `tftpboot`, `ping`, HTTP `wget`) and the board-params/MAC handoff to
Linux, see [network](#network-al_eth-built-not-yet-run-on-hardware). Staged in
`/srv/tftpboot/dmc12-uboot/eth-b/` (driver + handoff) and `eth-a/` (handoff only, no
network stack); result links `nixos-build/result-uboot-dmc12-eth-{a,b}`.

## what exists

- `nix build path:./nix#uboot-dmc12-chainload` → `u-boot.uimage` (for `bootm`),
  `u-boot.bin` (for `go`/`booti`), `u-boot`, `.map`, `.dtb`, `.config`.
  Current (with Ethernet, = `eth-b`): `/nix/store/5ivzlvicrzwnhyf7vpgja1zrk0s0kcva-…-2026.10`,
  `u-boot.uimage` sha256 `062428a9…`, `u-boot.bin` `f4b4b4f3…`. `eth-a` (0018+0019
  only, built at commit 7e438d0; the later 0018 change only matters with the driver):
  `/nix/store/2j3xfn9znz0wavzxg3b3j80k4dmbh102-…`, uimage `6e16f48b…`. The
  hardware-tested build (no network): `/nix/store/7w61vis50vjz9p5vjm4vs4czbn0wsjba-…`,
  uimage `7634a36c…`.
- Patches: shared series [ai_re/nix/packages/uboot/default.nix](../../nix/packages/uboot/default.nix):
  - 0014 `alpine-v2` (any Alpine V2 board): new target `TARGET_ALPINE_V2`, with no low-level init.
  - 0015 `dmc12`: CPLD sysreset driver.
  - 0016 `dmc12`: `-u-boot.dtsi` and `alpine_v2_dmc12_defconfig`.
  - 0017 `alpine-v2`: initrd and FDT used in place (`fdt_high`/`initrd_high`).
  - 0018 `alpine-v2`: al_eth board params + MAC handoff before Linux, `aleth` command.
  - 0019 `dmc12`: the port's DT node (00:01.0, rgmii fixed-link 1000, EEPROM MAC).
  - 0020 `alpine-v2`: the al_eth network driver (HAL copied verbatim from `packages/al-eth/src`).
  - 0021 `dmc12`: network on in the defconfig, no ethernet bootdev.

  The T208x builds take only their own scopes, and their `.drvPath`s are unchanged.
  The board DTS is the Linux one, `nix/packages/linux-alpine/boards/dmc12/alpine-v2-dmc12.dts`,
  copied in at build time. A DTS edit there rebuilds U-Boot.
- Tests: `nix build path:./nix#uboot-alpine-v2-tests` (no board needed, all three pass).
  - **static:** AArch64 ELF linked at 0x08000000, Linux Image header, uImage header,
    RAM footprint, no `smc`/`hvc` instruction anywhere. The config is checked to have
    no SPI flash, env storage, MMC, EFI, PSCI reset, ethernet bootdev or EEPROM-writing
    command, the sp805 not auto-started, and al_eth + dhcp/tftp/ping/wget present. The
    DT is checked for the memory banks, firmware ranges, CPLD and sp805 nodes, the
    500 MHz UART bound before relocation, and the al_eth port node with its EEPROM phandle.
  - **eth-params:** host build of the handoff encoders (`include/alpine_v2_eth.h` out of
    the series) against the HAL in `packages/al-eth/src`: register offsets, enums, the
    DMC-12 params `60000001 00000004 00000000`, 20000 random param sets through
    `al_eth_board_params_set/get`, MACs through `al_eth_mac_addr_store/read`, and the
    ONIE TLV parser (the CRC convention was also checked by hand on the u01 EEPROM
    dump). Also: the 23 HAL files inside patch 0020 are byte-identical to `packages/al-eth/src`.
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
    Also checked: the al_eth handoff runs before "Starting kernel" and reports no NIC.
    QEMU has no al_eth model, so the network driver is tested on hardware only.

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
- **Network:** see [network](#network-al_eth-built-not-yet-run-on-hardware). The default
  boot stays SATA only: there is no ethernet bootdev, so `bootflow scan` never DHCPs.

## load addresses

| range | owner |
| --- | --- |
| 0x0 +1 MiB, 0x02000000 +1 MiB | firmware (reserved) |
| ~0x03B00000–0x04000000 | factory U-Boot (stack 0x03B04CD0, `fdtaddr` 0x03B06010, relocaddr 0x03F0A000) |
| 0x04078000 +28 KiB | factory DTB copy (`loadaddr_dt`) |
| **0x08000000–0x080E3CB0** | **this image** (code + BSS + 512 KiB early stack; dead after relocation; with Ethernet) |
| 0x0f000000 / 0x0f100000 / 0x0f200000 / 0x0f300000 | `fdt_addr_r` / `fdtoverlay_addr_r` / `scriptaddr` / `pxefile_addr_r` |
| 0x0fd00000 | factory initrd staging (not used by us) |
| 0x10000000 | uImage TFTP staging (factory side), then `kernel_addr_r` |
| 0x20000000 +64 MiB | `kernel_comp_addr_r` (Image.gz) |
| 0x30000000 | `ramdisk_addr_r` |
| just below 0xb0000000 | relocated U-Boot, malloc, its FDT |

`fdt_high`/`initrd_high` = `0xffffffffffffffff` (patch 0017): `booti` uses the FDT and
initrd where they were loaded, no copy. The value must be 64-bit; `0xffffffff` would be
read as a 4 GiB limit.
| 0xb0000000 +256 MiB | firmware (reserved) |

## network: al_eth (built, not yet run on hardware)

The port is integrated Ethernet port 1: PCI 00:01.0 `1c36:0001` rev 2, RGMII at a fixed
1000/full to the CPLD-managed uplink PHY pair. It has no MDIO PHY and no SerDes.
BARs: 0 = UDMA (128 KiB), 2 = MAC (4 KiB), 4 = EC (16 KiB). U-Boot assigns them inside
0xfe000000 +16 MiB, and Linux moves them again.

**Handoff to Linux (patch 0018, also in builds without a network stack).** Linux
al_eth reads three "board params" scratch registers (MAC BAR +0x4, +0x404, +0xc) and the
MAC address (EC forwarding entry 0, +0x868/+0x86c). Without the params, probe fails
("board info not available"). Without the MAC, it picks a random one. An FLR clears
both. In `board_prep_linux` (just before "Starting kernel"), for every 1c36:0001
function:
- **Params** already there are kept; a difference from the DT is printed, not acted on.
  Missing params are written from the function's DT node (`phy-mode` + `fixed-link`),
  encoded as the HAL's `al_eth_board_params_set()` does. On the DMC-12 that is
  `60000001 00000004 00000000`: rgmii, no PHY, autoneg off, 1000, full, ref-clk
  500 MHz. ref-clk is inferred; only the MDIO divider uses it.
- **MAC**, first that exists: env `ethaddr`, what the EC already holds, the board
  EEPROM's ONIE base MAC (TLV 0x24, CRC checked; it equals the factory env's `eth1addr`).
- `aleth info` shows BARs, params and MACs (read-only). `aleth handoff` runs the
  handoff by hand.

**Driver (patch 0020).** The device is `al_eth1`, named like the vendor U-Boot's. It runs
on Annapurna's HAL, copied verbatim; only the platform shim is new, and its `al_assert`
reports instead of halting. The order follows Linux al_eth:
- **start:** first quiesce whatever ran before: MAC stop, all 8 queues reset, UDMA
  disable, FLR keeping params + MAC. Then adapter init, MAC config from the board
  params, link config 1000/full, one TX and one RX queue, RX forwarding (our unicast and
  broadcast, the rest dropped), 63 RX buffers of 1536 B, MAC start.
- **stop and remove:** both quiesce the same way. remove runs before the OS
  (`DM_FLAG_OS_PREPARE`), so Linux finds the function idle with params and MAC in place.
- **probe:** maps the BARs, runs the handoff, allocates. Nothing touches the data path
  before a network command, so a driver fault can't break a disk boot.
- **DMA:** cached memory with explicit flush/invalidate. TX goes through an aligned
  bounce buffer padded to 60 B. The MAC's link bit is printed at start but not trusted:
  Linux turns the carrier on unconditionally for this port.

Commands: `dhcp`, `tftpboot`, `ping`, `wget`. `wget` is the legacy stack's: HTTP only,
the server given as an IP, the port from env `httpdstp` (default 80). There is no env
storage, so nothing persists.

### hardware test plan (main agent; RAM only, nothing written)

Run each step only after the one before it passed. Start from an idle `ALPINE_DB>`
(`./powercycle_to_uboot.py`).

1. **Factory side, before any chainload** (read-only; answers question 1 in
   `uboot-vendor-commands.md`):
   1. `pci header 0.1.0` for the BARs.
   2. `md.l` MAC BAR +0x4, +0x404, +0xc and EC BAR +0x868, +0x86c.
   3. Repeat 2 after `setenv autoload no; dhcp`.

   Do the first reading straight after the power-cycle, before any `dhcp`. Expect params
   at or near `60000001 00000004 00000000`. Record the first word: it settles ref-clk.
   Don't paste the EC words (the MAC) into tracked files.
2. **Handoff only:** `./chainload_uboot.py nixos-build/result-uboot-dmc12-eth-a --cmd
   'aleth info::board params'`. Expect:
   - `board params (present): …` with the factory values;
   - a valid `EC MAC:` (the chainload ran `dhcp`), equal to the `EEPROM MAC:` line.

   Then `aleth handoff` must report "kept" for both.
3. **Driver probe:** the same with `…-eth-b`. Expect the banner, an `al_eth 00:01.0:`
   line and `Net:   al_eth1`, with no "random MAC" or "No valid MAC" warning.
   `aleth info` as in step 2.
4. **Data path:**
   - `setenv autoload no; dhcp`: expect `al_eth1: RGMII 1000M full, link bit …` and a
     lease in 10.60.2.0/24.
   - `ping 10.60.2.208`: expect "is alive".

   On failure, note what the link bit said and capture `aleth info`.
5. **TFTP:** `setenv serverip 10.60.2.208; tftpboot ${kernel_addr_r}
   dmc12-uboot/eth-b/u-boot.bin; crc32 ${kernel_addr_r} ${filesize}`. Compare with the
   host: `python3 -c 'import zlib,sys;print(hex(zlib.crc32(open(sys.argv[1],"rb").read())))' <file>`.
   Then repeat with a large file, `dmc12/uInitrd` (~300 MB), and note the rate U-Boot prints.
6. **HTTP:**
   1. On the host: `python3 -m http.server 8000 --bind 10.60.2.208 --directory
      /srv/tftpboot`. Any free port works; 8000 is free on the host today. Check the
      host firewall.
   2. At `=>`: `setenv httpdstp 8000; wget ${kernel_addr_r}
      10.60.2.208:/dmc12-uboot/eth-b/u-boot.bin`, then `crc32` as in step 5.
   3. Then the uInitrd, for the rate.
7. **Linux after U-Boot networking (the handoff end to end):** netboot NixOS as below.
   In Linux, expect:
   - `al_eth … Board info: phy exist No … media 1`;
   - the board MAC on eth0, not a random one;
   - eth0 up at 1000 with DHCP.

   Then `./smoke.py`.
8. **Disk boot unaffected:** `bootflow scan -l` lists only SCSI bootflows (no
   ethernet, no DHCP).

If step 4 gets no RX at all, the likeliest causes, in order:
1. RX forwarding: compare MAC entry 0 (`md.l <EC BAR>+0x868 2`) with the MAC.
2. RGMII clocking/delays: the factory U-Boot has `al_cmos_rgmii_dll_*` code that this
   HAL lacks (see risks).
3. Cache maintenance.

### netboot NixOS through it

The existing TFTP bundle boots unchanged from upstream U-Boot, which has `bootm` and
`source`. The bundle is `dmc12-netboot` in `/srv/tftpboot/dmc12/`: legacy uImage,
uInitrd, dmc12.dtb, boot.scr.

```
=> setenv autoload no; dhcp; setenv serverip 10.60.2.208; setenv dmc12dir dmc12
=> tftpboot 0x07000000 dmc12/boot.scr; source 0x07000000
```

Over HTTP, use the server from step 6 with the same files and the same addresses as
boot.scr. Take the bootargs from the bundle's `bootargs.txt`; the console buffer is
1024 B, so typing them works when paced (console.py).

```
=> setenv autoload no; dhcp; setenv httpdstp 8000
=> wget 0x10000000 10.60.2.208:/dmc12/uImage
=> wget 0x20000000 10.60.2.208:/dmc12/uInitrd
=> wget 0x04078000 10.60.2.208:/dmc12/dmc12.dtb
=> setenv bootargs '<contents of bootargs.txt>'
=> bootm 0x10000000 0x20000000 0x04078000
```

`bootm` moves the kernel to 0x04200000, the uImage load address, clear of everything
this U-Boot still uses. The natural next step for `nix/packages/dmc12-images` is a
raw-`Image` + `booti` bundle (kernel at `kernel_addr_r`, initrd at `ramdisk_addr_r`,
FDT at `fdt_addr_r`). It isn't built yet.

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

## on-disk boot (built, not yet run on hardware)

`hardware.dmc12.bootLoader = "chainload"`
([boot-chainload.nix](../../nix/modules/boards/dmc12/boot-chainload.nix),
[disk.nix](../../nix/modules/boards/dmc12/disk.nix)); output
`path:./nix#dmc12-disk-image-chainload`. The default image stays `uboot-menu`.
The factory U-Boot never reads ext4 in this mode (its ext4 is flaky, TFTP is slow).

| part | LBA / size | content |
| --- | --- | --- |
| sda1 | 2048, 8 MiB, MBR type `0xda` | raw: U-Boot uImage slot A at LBA 0x800, slot B at 0x2800 (4 MiB each) |
| sda2 | 18432, 512 MiB, bootable | ext4 `DMC12BOOT` (default features) = /boot: `extlinux/extlinux.conf`, `nixos/*` |
| sda3 | after it | ext4 `DMC12ROOT` = /, grown on first boot |

1. **Factory U-Boot** env (reimage.py writes it from the image's `factory-env.txt`):
   `bootcmd=run nixos_boot; run dmc12_bootdisk` and
   `nixos_boot=run dmc12_boot_preamble; scsi init; scsi read 0x10000000 0x800 0x2000 && iminfo 0x10000000 && bootm 0x10000000 - ${fdtaddr}; <same for 0x2800>`.
   The preamble is what the factory boot runs (DN0 SATA SerDes fix, WDT flag, fans 70%,
   LED, uplink sync, `bootp` with `autoload=n`): `bootp`'s al_eth halt leaves the board
   params + MAC that Linux al_eth needs (`uboot-vendor-commands.md`). `scsi read` takes
   hex address, start block, count. Both slots bad → falls through to `dmc12_bootdisk`.
   The `setenv` line is 233 chars typed; an eval assertion keeps it under 254.
2. **`bootm 0x10000000 - ${fdtaddr}`**: the same entry as the hardware-tested TFTP path.
3. **Upstream U-Boot**: `bootflow scan -lb` → sda2 (bootable flag) →
   `/extlinux/extlinux.conf` from nixpkgs' extlinux builder: raw `Image` at
   `kernel_addr_r` 0x10000000, initrd at 0x30000000, `FDT ../nixos/…-dtbs/amazon/alpine-v2-dmc12.dtb`,
   3 s menu, 10 generations → `booti` at EL2.

Updates: the installer (activation) rewrites /boot via the extlinux builder, then, if
the U-Boot image changed, copies slot A to B and writes the new image to A. It refuses
unless sda1 is exactly this partition (start, size, type `0xda`). The disk image has
the same image in both slots.

- **Checked offline** (2026-11-15): see PROGRESS entry; partition table, both slot
  headers (`dumpimage -l`), extlinux.conf, root closure.
- **Not yet run:** factory `scsi read` of our slot; Linux via `booti` from disk;
  al_eth after the preamble's `bootp` on this path.
- **Recovery:** the factory countdown (5 s) still comes before `bootcmd`; from that
  prompt `run dmc12_bootdisk` or the TFTP chainload above. A U-Boot that hangs after
  `bootm` can't fall back by itself: power-cycle and interrupt.

## risks / unverified

- What has run on u01 is in the status at the top (console, I2C/CPLD, PCI, AHCI, CPLD
  reset via `bootm`). Everything else here is build- or QEMU-tested only.
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
- **On-disk boot:** the DN0 SATA SerDes write comes from `dmc12_boot_preamble`, which
  `nixos_boot` runs first (`uboot-vendor-commands.md`). al_eth's board params and MAC
  come from it too, and now also from the upstream handoff (patch 0018) whenever they
  are missing. Unverified on hardware. Without a DHCP answer, `bootp` times out first,
  which only slows the boot.
- **Network (patches 0018–0021), none of it run on hardware:**
  - RGMII timing. The factory U-Boot contains `al_cmos_rgmii_dll_start` /
    `_delay_set`, which this HAL (and Linux al_eth) lacks. Linux works after the
    factory's init, and so should this, if the DLL state survives the factory's FLR the
    way it does for Linux. A board whose NIC the factory never touched is untested.
  - DMA coherency is not assumed. Rings and buffers get explicit cache maintenance; a
    bug there shows as RX garbage or TX timeouts, not a hang.
  - ref-clk in written params is inferred (500 MHz). Firmware values win when present.
  - Legacy `wget` is HTTP only, and nobody has measured whether it is fast enough for a
    300 MB initrd. TFTP uses blocksize 1468, window 1.
  - The control DT now has a `/soc/pci@fbc00000/ethernet@1,0` node. A Linux booted
    with U-Boot's own DT sees it; the out-of-tree al_eth ignores DT.
