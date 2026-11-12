# Delta TN48M

Details: [uboot.md](uboot.md) (password, signature enforcement, commands,
recipes), [dentos.md](dentos.md) (as-received disk + provenance),
[ihm.md](ihm.md) (Amazon IHM device identity / credential system, passive map),
`onie-delta_tn48m-r0.dts` / `dentos-delta-tn48m.dts` (decompiled vendor DTBs),
`onie-env-dmesg.txt` (full U-Boot env + ONIE dmesg).

## current status (2026-11-11)

- Root on ONIE (serial, no login). **U-Boot password `@wa23`**, verified.
- SPI (16 MiB) and the whole SSD (27.5 GB) dumped; hashes verified against
  on-device sha256. Nothing has been written to SPI, env or SSD.
- Unit is an Amazon DENT SKU, factory-provisioned 2021-07-21, never configured.
- U-Boot enforces RSA-signed FIT configs (Amazon `ihm_dent_image_signing`).
  `booti`/extlinux raw Image/`bootefi` aren't checked, so the NixOS route works.
- ONIE's DTB lacks the ATF reserved-memory node and **crashes under memory
  pressure**. For bulk I/O, boot it with the RAM-patched DTB (uboot.md).
- SoC is **secure-boot fused**, so SPI U-Boot can't be replaced. U-Boot 2018.03's EFI
  is broken (log (c)), so EFI comes from a chainloaded mainline U-Boot (log (e)).
- **NixOS boots unattended over real UEFI**: factory U-Boot 2018.03 ->
  chainloads mainline **U-Boot 2026.10** (`/u-boot-tn48m.bin` on the ESP, built
  by `nix/nixos/module/tn48m/uboot`) -> bootstd EFI -> **systemd-boot 262** ->
  Linux 7.3.3. Cold power cycle verified 2026-11-12. SPI env `nos_bootcmd` =
  `scsi reset; load scsi 0:1 0x7000000 u-boot-tn48m.bin && dcache off && icache off && go 0x7000000; setenv ramdisk_addr_r 0x10000000; sysboot scsi 0:1 any 0x0f000000 /extlinux/extlinux.conf`
  (the extlinux tail is a fallback regenerated on every switch for the current
  generation; undo everything with `fw_setenv nos_bootcmd true`).
  Mgmt end2 DHCP 10.60.2.218 (MAC from the TLV EEPROM via U-Boot's DT fixup), on BasementMDFSwitch1
  USW-Pro-Max-48-PoE port 4 (LLDP).
  Deploy: `sudo nix copy --no-check-sigs --to ssh-ng://root@10.60.2.218 <toplevel>`,
  then `nix-env -p /nix/var/nix/profiles/system --set <toplevel>` +
  `<toplevel>/bin/switch-to-configuration boot|switch` over `sudo ssh` (root key of sea420-desktop).

- Switch is sitting in ONIE (patched-DTB boot), mgmt 10.60.2.217.

## identity

- device: Delta Networks (DNI) **TN48M** (plain, not -P/PoE), 48x 1G + 4x 10G SFP+.
- chassis S/N `TN481N1TW21290148`, service tag `3811100345`, mfg 2021-07-16,
  device version 3, label rev D1, base MAC `18:BE:92:16:56:40` (64 MACs),
  diag V1.4.0, ONIE 2019.08-V03 (ONIE syseeprom TLV, CRC valid).
- SoC: Marvell **Armada 7040-B0** (AP806-B0 + CP115-A0), 4x Cortex-A72
  (`0x41/0xd08` r0p1) @ 1400 MHz; DDR4 (`mv_ddr4_*` training) @ 800 MHz.
- RAM: **8 GiB** (bank0 0x0+3 GiB, bank1 0x1_0000_0000+5 GiB). Vendor DTBs
  say 2 GiB; U-Boot fixes the memory node up at boot.
- SPI NOR: Winbond **w25q128bv, 16 MiB** on cp0 spi0 CS0. TPM: Infineon
  SLB9670 (TPM 2.0) on spi0.
- SSD: M.2 SATA `M.2 (S42) 3TE7`, fw `S21524D`/rev `24D`, S/N-ish
  `B2A12103250022502` (smartd state), 53,742,528 sectors (27,516,174,336 B), comphy1.
- Switch ASIC: Marvell Prestera **98DX3265 (AC3X)**, PCIe `11ab:c80c`, Gen2 x1 on
  PEX0. Mainline `prestera` lists `0xC80C`. Firmware is linux-firmware
  `mrvl/prestera/mvsw_prestera_fw-v4.1.img`.
- SerDes (U-Boot comphy): 0 PEX0 5G, 1 SATA0 6G, 2 SGMII0 3.125G, 3 SGMII1 1.25G,
  4 unconnected, 5 SGMII2 1.25G. mvpp2 eth0/eth1/eth2; **eth2 = OOB mgmt**
  (88E1512 PHY).
- I2C: cp0 i2c0 = EEPROM 0x56 (TLV), ADT7473 0x2e; tmp1075 0x4a/0x4b; AP i2c
  (`i2c@511000`) = CPLD 0x41. Fans 1-3, single PSU (onlpd).
- EEPROM "Platform Name" TLV `88F7040/88F6820`. The 88F6820 (Armada 38x) is in
  the CPLD reset map; its role is unknown.

## access

- serial: front console, **115200 8N1 confirmed 2026-11-11**, FTDI FT232R
  `B0016Y5Q` (same cable as the cs8210). tmux session `serial-tn48m`, logged to
  `serial-YYYYMMDD-HHMMSS.txt` here. Levels not measured by the agent.
- power: `tools/lab_power.py` (same HA lab plug as the cs8210).
- U-Boot: `@wa23` during "Type password to STOP autoboot" (10 s) -> `Marvell>>`.
- ONIE: root shell on serial. The mgmt port DHCPs (10.60.2.217, MAC ...:42).
  dropbear 2016.74 rejects root ("Login attempt for nonexistent user") even with
  a key. Bulk transfer: switch `nc -l -p PORT < file`, host
  `nc 10.60.2.217 PORT > file`. Busybox nc doesn't close at EOF: Ctrl-C it on
  the switch, then kill the host nc so the pipeline flushes.

## boot chain

ATF BL1/BL2/BL31 v2.2 (Marvell-10.0.0, built 2021-03-01; BL31 at
0x4023000-0x402f198) -> U-Boot `2018.03-devel-1.2.0 ... V06_test` ->
`bootcmd=run bootonie` -> `nos_bootcmd` (**`true`** as received) -> ONIE FIT
from SPI 0x400000 (+0xC00000) via `bootm` (signed, verified). Env in SPI at
0x3f0000 (64 KiB, `/dev/mtd1`).

SPI layout: mtd0 `uboot` 0x000000-0x3effff, mtd1 `uboot-env` 0x3f0000-0x3fffff,
mtd2 `onie` 0x400000-0xffffff.

## dumps (in `dumps/`, binaries gitignored)

| file | size | sha256 (matches on-device) |
| --- | --- | --- |
| mtd0.bin (uboot) | 0x3f0000 | `53de70261925b655382edabcec784ab2911556d9ec71393e14620e5fbd9452fd` |
| mtd1.bin (env) | 0x10000 | `77e8c68e8a36b0274a3dc71e78926a946e4bd128dd3087f249141434ca527015` |
| mtd2.bin (onie) | 0xc00000 | `7d45c084951e45725a12828d0d1d23ec1a8a1b1273b5526085922c2cd6114d54` |
| sda.img.zst (raw sda, zstd -3) | 27,516,174,336 raw | `a7d658a888f193c6b96ed5181e9d3357c9a65ec3e894fbabab54337ec40721dd` (raw) |

`sda-partial-8GB-crashed.img.zst` is the first 8.07 GB from the crashed
attempt; delete it whenever.

## NixOS

Baseline: `github:andrewhamon/delta-tn48m` @0ab0f8d (MIT, 2026-09-05; tested
on TN48M-P), vendored into `nepeat/nix` as `nixos/module/tn48m` (hardware,
DTS, dtb builder, thermal, watchdog) + host `nixos/machine/tn48m`, flake
output `tn48m`. The user wants rolling unstable: nixpkgs `master` (26.11pre,
rev 370746b at build time) + `linuxPackages_latest` (7.3.3). The stock cached
kernel has every needed driver, and linux-firmware 20261019 ships
`mrvl/prestera/mvsw_prestera_fw-v4.1.img`, so there's no custom kernel.
Changes vs upstream: no PoE/PMBus, only the Prestera firmware dir shipped,
fan floor PWM 96 (stock fans, unmeasured), watchdog off during bring-up,
networkd DHCP on `end2` only (front ports unmanaged = admin-down), root ssh
keys as on cs8210, serial autologin, and the cs8210 lldpd aarch64 patch.

Built 2026-11-11: `nix build path:$PWD#nixosConfigurations.tn48m.config.system.build.sdImage`
-> `nixos-tn48m.img` 4,199,673,856 B. MBR: p1 FAT stub (16384+8192), p2 ext4
`NIXOS_SD` (24576+..., bootable) with `/boot/extlinux/extlinux.conf` (raw
`Image` 72.8 MB, initrd 27.7 MB, `FDT .../marvell/delta-tn48m.dtb`). Not
installed yet.

Install plan (**needs user OK**: it destroys the SSD's dentOS, which is backed up
in `dumps/sda.img.zst`, and writes one SPI env var):
1. Boot ONIE with the patched DTB (uboot.md); `nc -l -p 9040 | dd of=/dev/sda bs=1M`
   on the switch, `nc -N 10.60.2.217 9040 < nixos-tn48m.img` on the host; then `sync`.
2. First boot by hand from U-Boot (RAM only): `setenv kernel_addr_r 0x07000000;
   setenv fdt_addr_r 0x06000000; setenv ramdisk_addr_r 0x10000000; scsi reset;
   sysboot scsi 0:2 any 0x10000000 /boot/extlinux/extlinux.conf`.
3. If it boots: persist it as `nos_bootcmd` (one env var; `fw_setenv nos_bootcmd true`
   reverts to ONIE).

## mistakes / gotchas

- Streaming /dev/sda through ONIE with the stock DTB panicked the kernel (see
  log). Always use the patched-DTB boot for anything big.
- Typed ahead into U-Boot while `sf read` ran; part of the input was dropped.
- The first ro mounts of sda1-4 lacked `noload`. The ONL-DATA superblock still
  shows last mount 2022-05-11 and clean, so it appears nothing was replayed.

## log (newest first)

### 2026-11-12 (f) — loose ends from (e) tied up

- **"scsi 0:2" U-Boot crash, root-caused**: rebuilt the crashing v1 bit-identically
  (sha256 66416bd0...) with its ELF. elr 0x3eeb8 = `mvpp2_bm_pool_use` <-
  `mvpp2_swf_bm_pool_init` <- `mvpp2_port_init` <- `mvpp2_probe`, during
  bootstd's lazy ethernet-bootdev probe (not ext4). FAR 0x4f300000edfe0dd0 =
  bytes `d0 0d fe ed 00 00 30 4f`, an FDT header, so a stale mvpp2 controller
  pointer into reused memory on re-probe (inferred, not traced). Fix: networking
  compiled out of the chainload U-Boot (`CONFIG_NO_NET`), which also needed
  (in the patch) PXE/DHCP distro targets guarded on CMD_PXE/CMD_DHCP, a guard for
  `CONFIG_ENV_MMC_DEVICE_INDEX` in `mmc_get_env_dev()`, and dropping `bubt`
  (SPI flash writer; a brick on this fused SoC anyway).
- **MAC from the EEPROM**: U-Boot DTS got `eeprom@56` (atmel,24c64) on cp0_i2c0;
  `CONFIG_ID_EEPROM` + `CMD_TLV_EEPROM` -> `mac_read_from_eeprom()` sets
  ethaddr/eth1addr/... from the TLV base + count; the EFI DT fixup
  (systemd-boot -> EFI_DT_FIXUP) writes them to the Linux DT's ethernetN aliases:
  ethernet-port@0/1/2 = :40/:41/:42, as the factory U-Boot did. networkd MAC pin
  removed; end2 keeps :42 and 10.60.2.218.
- **extlinux fallback** no longer stale: systemd-boot `extraInstallCommands`
  runs nixpkgs' extlinux-conf-builder (`-g 1`) for the generation being
  installed (/boot/extlinux + /boot/nixos, ~100 MB; ESP 43% used).
- **crypto-safexcel**: ship `inside-secure/eip197_minifw` -> EIP197 probes and
  registers its algorithms. `hardware.wirelessRegulatoryDatabase = true`
  silences the cfg80211 regulatory.db miss.
- Verified benign, left alone: `cpufreq-dt is already registered` (scaling works,
  350/467/700/1400 MHz); `TPM error (256) ... self test` (nothing runs
  TPM2_Startup before Linux; the TPM works: IFX, tpm2_getrandom OK).
- Cold power cycle hands-off with the new U-Boot (689 KB): systemd-boot menu ->
  gen 5 -> login, 52 ports, 0 failed units. Removed ESP leftovers (`/dtbs`,
  temporary `u-boot-good.bin`).

### 2026-11-12 (e) — legit EFI: chainloaded mainline U-Boot + systemd-boot

- Built mainline U-Boot 2026.10 (nixpkgs master) from `mvebu_db_armada8k_defconfig`
  + `nixos/module/tn48m/uboot/0001-...patch`:
  - new `armada-7040-tn48m.dts` (U-Boot control DT): factory AP/CP pinmux and
    comphy lanes copied verbatim from the factory control DTB (numeric
    phy-types are identical to mainline's constants), cp0_spi0 NOR, SATA0,
    USB2 (usb3_0 + utmi0), MDIO PHY @0, mvpp2 eth2 SGMII. Compatible
    `delta,tn48m`, so the DB-only PCA9555 VBUS code in board.c is skipped.
  - **DRAM mapping fix**: upstream `mvebu_mem_map` maps RAM only up to
    ATF_REGION_END + 2 GiB, but `lmb_arch_add_memory()` gives all banks to LMB
    -> EFI conventional memory, and AllocateAnyPages takes the top, so the same
    crash class as the factory U-Boot on any >2 GiB board.
    `a8k_mem_map_add_dram()` (called from `a8k_dram_init_banksize()`, which
    runs before `arch_reserve_mmu()`) adds the rest of bank 0
    (0x8540_0000-0xC000_0000) and bank 1 (0x1_0000_0000 + 5 GiB). Upstreamable.
  - Config: POSITION_INDEPENDENT, ENV_IS_NOWHERE (never touches the SPI env),
    SPI_FLASH_WINBOND, SCSI/AHCI, FAT_WRITE + EFI_VARIABLE_FILE_STORE
    (`/ubootefi.var`), no extlinux bootmeth, `bootcommand = bootflow scan`.
- Chainload: `load scsi 0:1 0x7000000 u-boot-tn48m.bin; dcache off; icache off;
  go 0x7000000` from the factory prompt (factory `dcache off` also turns off the
  MMU). First try: SATA, USB, comphy all fine; EFI ran systemd-boot correctly
  (it complained only about missing entries). Problems fixed after that:
  extlinux bootmeth picking up the stale ESP extlinux.conf (DB env load
  addresses overlap: "RD image overlaps OS image"); `bootflow scan -lb` needs
  BOOTSTD_FULL; mvpp2 "No valid MAC address found" (no env), so Linux end2
  MAC is pinned via networkd `[Link] MACAddress=18:be:92:16:56:42`.
- Seen once: after EFI fell through, bootstd scanning `scsi 0:2` (ext4 root)
  took a synchronous abort in U-Boot (far 0x4f300000edfe0dd0, reloc elr
  0x3eeb8). Not reached now that EFI boots from p1; the U-Boot ELF is now
  installed for symbolizing. **Open.**
- NixOS: `hardware.tn48m.boot = "efi"`: systemd-boot (graceful, no EFI var
  writes, installs the DTB `devicetree` line -> U-Boot EFI_DT_FIXUP),
  u-boot-tn48m.bin via `systemd-boot.extraFiles`. `bootctl status`: Firmware
  UEFI 2.110 (Das U-Boot), systemd-boot 262; EFI memmap gives Linux all 8 GiB
  (EFI tables at 0x23ff..., i.e. top of bank 1 is mapped).
- Cold power cycle, hands-off: BootROM -> ATF -> U-Boot 2018 -> U-Boot 2026.10 ->
  systemd-boot -> 7.3.3 -> login; 52 ports, fancontrol active, 0 failed units.
- Benign boot noise: `tpm tpm0: A TPM error (256) ... self test` (dentOS
  showed it too), `crypto-safexcel ... error -2` (EIP197 firmware not shipped,
  since only Prestera firmware is included), `cpufreq-dt is already registered`.

### 2026-11-11 (d) — extlinux boot persisted

- Host `tn48m` switched to `hardware.tn48m.boot = "extlinux"` (extlinux.conf on
  the ESP, /boot = p1 vfat; configurationLimit 3, since ~100 MB per kernel on
  512 MiB). Built on sea420-desktop, copied with `nix copy --no-check-sigs` (the
  remote daemon rejects unsigned paths otherwise), `switch-to-configuration boot`.
  Removed the EFI-test kernel/initrd copies from `/boot/EFI/nixos`.
- `fw_setenv nos_bootcmd '...sysboot scsi 0:1 any 0x0f000000 /extlinux/extlinux.conf'`
  from NixOS (first SPI env write on this unit). Hands-off `systemctl reboot`:
  BootROM -> ATF -> U-Boot -> sysboot (conf, initrd, Image, DTB) -> 7.3.3 ->
  login, generation c6dcws5x..., 0 failed units.

### 2026-11-11 (c) — secure boot, EFI attempt, NixOS boots (booti)

- **SoC is secure-boot fused**: BootROM prints `Secure boot from SPI NOR flash 0
  (0x34)`. mtd0's Marvell header has ext_count 2: BINARY (BLE/mv_ddr) at 0x40 and
  a SECURITY ext at 0x5c000 (9956 B: RSA-2048 KAK, sha256 of DER
  `6695e021480f4e48942f8be2423dc3dff634c65f0d8da2dc9516020f42425467`; CSK slots
  0-3; jtag_en=1, encrypt_en=0, box_id 0xde8de800, flash_id 0xc0dec0de). The
  signed boot image (BL1 + FIP incl. BL33) is verified by the BootROM, so **SPI
  U-Boot can't be replaced** (no key). Efuse bits themselves not read.
- With user OK, **clobbered dentOS** (backup verified earlier): wrote the
  EFI-layout NixOS image (`nixos-tn48m.img`, 4,730,032,128 B, sha256
  `421b3f654b04ff0dd6e3814027e7674e883679186ccffc6f0aee74afb9b0388e`, MBR
  p1 ESP type EF 512 MiB, p2 ext4) from patched-DTB ONIE over nc; readback
  hash matches.
- **U-Boot 2018.03 EFI is broken** (all tests RAM-only, nothing saved):
  1. Its MMU maps RAM only up to 0x1_0540_0000 (L1[0..2] + an L2 table
     for the first 84 MiB of bank 1), but the EFI memory map advertises all of
     bank 1 (to 0x2_4000_0000). Top-down EFI allocations fault (kernel stub
     relocation to 0x2_3b80_0000: esr 0x96000045). **Workaround**: fill the empty
     L2 entries (2 MiB blocks to 0x1_4000_0000) and L1[5..8] (1 GiB blocks)
     with `mw.q` (attrs 0x711, the same as U-Boot's). Replacing the live L1[4]
     table entry doesn't work (walk cache, no TLBI: L2 translation fault).
     Commands in `tools/uboot-map-high-ram.txt` (send with `tools/uboot-send.sh`); after that,
     reads to 0x2_3fff_fff0 work.
  2. systemd-boot (systemd 262) aborts reading 0x2_408c_0040 (past RAM). Same
     registers with and without the MMU fix. Likely the bogus
     `LoadedImage->ImageBase` the kernel stub reports ("FIRMWARE BUG:
     efi_loaded_image_t::image_base has bogus value").
  3. Kernel EFI stub direct (after fix 1): `Failed to load initrd:
     0x8000000000000009`, `Failed to install memreserve config table`,
     `Unable to construct new device tree` -> `Failed to update FDT and exit boot
     services`; the app returns r=1.
  => EFI needs a newer U-Boot (chainloaded), or extlinux/booti.
- **NixOS booted with plain `booti`** from the ESP files (kernel 0x7000000, raw
  initrd 0x10000000:size, DTB 0x6f00000, bootargs with init=). Kernel 7.3.3, all
  8 GiB, end2 DHCP 10.60.2.218 (EEPROM MAC ...:42, so U-Boot's fixup works), 52
  Prestera ports (fw loaded, admin-down), fancontrol at PWM 96 (fans ~4.5k
  rpm), temps 35-40 C, root grown to 25 GB, no failed units.
- Not persistent: SPI env still `nos_bootcmd=true` (next power cycle -> ONIE).
  Don't `nixos-rebuild switch` until the loader choice is made (the current
  config would install systemd-boot, which this U-Boot can't run).

### 2026-11-11 (b) — U-Boot password, patched-DTB ONIE, full disk, dentOS

- Power-cycled the panicked box. U-Boot `@wa23` worked first try (found as the
  only compiled-in stop string next to `bootdelaykey`/`bootstopkey`).
- Captured `help`, `version`, `bdinfo`, `coninfo`, `i2c bus`, `pci`, `config`
  -> `uboot-reference-raw.txt`, `uboot-config.txt`, summary in `uboot.md`.
- Found the required FIT signature key in U-Boot's control DTB (uboot.md).
- Booted ONIE from U-Boot with its DTB patched in RAM (psci-area no-map).
  Kernel showed 8386560K. Full 27.5 GB disk read over nc with no fault;
  host and device sha256 match. This supports the reserved-memory diagnosis.
- Analysed the image offline -> `dentos.md`.

### 2026-11-11 (a) — first power-up, root, SPI dump, ONIE panic

- Outlet on, caught the banner. Spaces didn't stop autoboot. ONIE came up in
  "NOS install mode"; ran `onie-discovery-stop`.
- Root shell on the ONIE console. ssh to dropbear failed (see access).
- SPI dumped over nc, all 3 hashes match on-device.
- Whole-disk stream (with a concurrent on-device sha256) **crashed ONIE**:
  SError on CPU0 at 741 s (Comm nc), a synchronous external abort at VA
  `ffff800004026008` in `__tlb_remove_page_size`, then a fatal oops in
  `mv_pp22_poll -> eth_type_trans` (skb at `ffff80000402fc00`), "Kernel panic -
  not syncing: Fatal exception in interrupt" at 814 s. It stopped after ~8.07 GB
  (= RAM size), so the page cache had filled memory.
- Diagnosis: ONIE's DTB (generic A7040 DB tree) has no `reserved-memory`.
  Mainline `armada-ap80x.dtsi` reserves `psci-area@4000000` (2 MiB, no-map)
  and `tee@4400000`. Both faulting addresses map to phys 0x4026008/0x402fc00,
  inside psci-area, so Linux allocated ATF secure memory once RAM filled.
  Confirmed by the patched-DTB run in (b).
- Device-side changes: only ONIE ramdisk (`/root/.ssh/authorized_keys`, `/tmp`).
