# DMC-12-48R-AS0: archived PROGRESS text

## Status bullets of the morning of 2026-11-15 (moved verbatim 2026-11-15 19:45; superseded)

- NixOS: generic image `nix/configs/dmc12` on `boards/dmc12` + `platforms/alpine-v2` +
  `packages/{linux-alpine,al-eth}` (mainline 7.3.4, new DTS, CPLD reboot hook,
  interrupt monitor). Outputs `dmc12-netboot` (TFTP RAM boot) and `dmc12-disk-image`.
  Reimaging: `reimage.py` / `reimage.md`. **RAM-booted on u01 2026-11-15** (TFTP, kernel
  variant `trimmed`): system `running`, 0 failed units; eth0 al_eth 1G up via DHCP; 48 cp210x +
  /dev/P1..48; I2C numbering = factory; lm75/tmp75 read; GPIO lines named; both SPI NOR
  read-only; sp805 bound; fancontrol PI holds step 21 (3200 RPM); interruptmon logs + masks
  the PSU1 alarm. Disk install not done yet. Raw report: `inventory/u01/nixos-first-boot-private.txt`.
  Open from that boot: kernel was misaligned at 0x04080000 (now 0x04200000); `show power`
  INA219 on i2c-7 reads 0 (unverified); tpm_i2c_infineon "could not request locality";
  al_eth "serdes regs base failed" (harmless, RGMII).
- 2026-11-15 round 2 (lean kernel, 1454 modules): `smoke.py --reboot` **18/18**. Reboot is a full
  CPLD board reset from the kernel module `dmc12_cpld_reset` (reboot notifier; WDT flag 0x03 after).
  A systemd-shutdown hook never ran in NixOS's exitrd. Fan policy uC 60 C / board 45 C holds the 2000 RPM
  floor (uC ~53 C). node_exporter + dmc12-metrics serve (182 dmc12_* series); `dmc12_cpld_alarm`
  reads 0 for alarms interruptmon has masked (PSU1), so use `dmc12_psu_present`. Chainload
  U-Boot with patch 0017: fdt/initrd_high 64-bit, coherency regs 0xf0090004=7, 0xf0094000=0xc0000001.
- 2026-11-15 round 3: **upstream U-Boot with al_eth works on u01** (eth-b, patches 0018-0021,
  chainloaded from RAM): dhcp, ping, TFTP 8.6 MiB/s and HTTP `wget` ~27 MiB/s (278 MB initrd in
  ~10 s), both CRC-exact vs the host. NixOS booted from HTTP-loaded files: smoke 15/15, Linux eth0 =
  board EEPROM MAC, al_eth board info present. Board params (60000001 00000004 00000000) and the EC
  MAC are already set at the factory prompt right after power-up, before any dhcp, so on this unit
  the "handoff gap" does not occur; U-Boot's handoff keeps them. Host firewall: use HTTP port 8000
  or 8080 (open); 8608 is not.
- Monitoring (built + offline-tested, not yet imported or run on u01): `boards/dmc12/monitoring.nix` =
  upstream node_exporter :9100 + read-only `dmc12_*` producer (`nix/packages/dmc12-metrics`), Grafana `ai-re-dmc12`.
- **Upstream U-Boot 2026.10** chainloads into RAM from the factory `bootm` and works on
  u01 (console, I2C/CPLD, PCIe, AHCI/SSD, CPLD reset): `u-boot-port.md`, `chainload_uboot.py`.
  Nothing flashed. On-disk boot through it (raw sda1 U-Boot slots, extlinux on sda2):
  `dmc12-disk-image-chainload`, built + offline-checked, not yet written to an SSD.
- **Left in:** the rdinit shell, powered on, at 10.60.2.10, with nothing mounted from disk.
  So no ser2net, and the AS7760's console on port 48 isn't served. A
  power-cycle returns it to the factory OS.
- **How this unit differs from the first iteration's:** see "This unit vs the
  first iteration" below. In short: it was deployed in a fleet, boots an older OS
  (1.0.457, while the old module ported ≥1.0.526), and only PSU2 is detected.
- **Images:** `ai-re-images` (tools/README) catalogs, tags and verifies every dump across devices;
  tags for this device are in `images.jsonl`. `export-qcow2` makes a zstd qcow2 for read-only nbd attach.
- **Units:** many more boxes are coming in. `units.md` has per-unit versions and the
  collection steps; serials are in `units-private.txt` (ignored). This box is **u01**.
- **K2 uC + factory U-Boot internals** (for future U-Boot patches): `k2-uc.md`.
- **Factory U-Boot vendor commands** (disassembled), what the boot path does for Linux, and
  port verdicts: `uboot-vendor-commands.md`. Before any on-disk boot, read its Ethernet
  board-params gap. Also: never reset the env, and uC GPIO 2.2 is the likely CS1 select.
- **Factory hardware tooling vs the module:** `vendor-tooling.md` (CPLD register map,
  reboot via CPLD/sp805, interrupt monitor, uplink re-sync, GPIO lines). Read it before
  reworking `board-dmc12`.
- Reboot: the **CPLD reset works** (0x29←1, 0x28←1 from Linux: full board reset, stage2 in
  under 1 s, U-Boot in about 7 s). Whether mainline's own `reboot` works is still to test.
- U-Boot netboot: `setenv autoload no; dhcp` (lab LAN lease) reaches the TFTP host
  10.60.2.208. No env writes needed for RAM boots.
- Open: what selects CS1 (mtd1), the PSU1 alarm (CPLD IRQ line gpio465 is held low by it).
  **Next kernel:** trim to allnoconfig + explicit (as linux-qoriq-t2080), from the drivers
  the first boot binds. Owner: the full nixpkgs module set is unnecessary.
  **After NixOS:** back up and analyse the CPLD (MachXO3 JTAG readback, cf. `cpldupd`), the uC
  firmware and the SPI `APCEA`/`APCEA_CFG` objects.

## PROGRESS log entries, 2026-11-15 up to 13:00 (moved verbatim 2026-11-16)

- 2026-11-15 13:00: chainload disk image finished (`u-boot-port.md` "on-disk boot").
  `hardware.dmc12.bootLoader = "chainload"` → output `dmc12-disk-image-chainload`
  (`/nix/store/9kfqi1zd…-dmc12-disk-image`, disk.img sha256 `f5e6c1ea…`, U-Boot uImage
  `3851ae72…`, new because patch 0017 sets 64-bit `fdt_high`/`initrd_high`). No kernel
  rebuilt. Offline: MBR sda1 type da 2048+16384, sda2 18432+1048576 bootable, sda3;
  `dumpimage -l` on both slots = the uImage (byte-equal); extlinux.conf names the raw
  `Image`, initrd, `FDT …/amazon/alpine-v2-dmc12.dtb`; root ext4 DMC12ROOT holds the
  toplevel, both fs `e2fsck -n` clean; QEMU alpine-v2 tests pass ("Using Device Tree in
  place"). uboot-menu image: same `.drvPath` as before. The factory env now runs
  `dmc12_boot_preamble` first (al_eth params via `bootp`). reimage.py/md changes for the
  image's `factory-env.txt` are with the main agent. Dropped the previous agent's
  unfinished QEMU network stage (needed a test e1000 + www dir, not wired up).
- 2026-11-15 03:40: factory tooling pass (`vendor-tooling.md`). Found the CPLD reboot
  driver (0x29/0x28, disassembled from the vendor kernel via vmlinux-to-elf), the
  interrupt monitor + GPIO lines, and that uplink re-sync is ≥526-only. Kept the factory OS
  images, symbol ELF and burn-in logs in `dumps/u01/factory-os/`. Moved everything to
  per-unit `dumps/u01/` and `inventory/u01/`, added `units.md`.
- 2026-11-15 03:20: inventory + offline analysis (above). Also learned: plain `.zst`
  can't be mounted. nbdkit 1.48 has no zstd decompress filter, but its `qcow2dec` (nixpkgs build) and
  qemu read zstd-compressed qcow2, so that's the random-access format (runbook).
- 2026-11-15 03:00: SSD image done. The first run verified the raw image, but its zstd step
  (`-T0`) died at 02:56 in a host-wide OOM: a `cc1plus` nix build plus zstd. The OOM
  killer took `sd-pam` and with it the whole user session (tmux, tio, every Claude
  session). Recompressed from the re-hashed raw with `acquire.py --compress` (now
  `-T6`, niced). The box stayed in the rdinit shell throughout (uptime >30 min, so the
  uC watchdog doesn't fire there). Then: I2C sysfs map, `hdparm -I`, live DTB,
  EEPROMs.
- 2026-11-15: info-gathering pass. Moved the FTDI session over from `serial-7760`
  (told that session; the AS7760 is SSH-only now). The first power-cycle missed
  autoboot because of tio log buffering; the helper now watches the pane. Got into
  U-Boot, saved the env privately, ran read-only `uc_get_*` and the flash TOC, booted
  rdinit shell, imaged mtd0/mtd1 (triple-hashed, zstd). Imported the first
  iteration's NixOS module verbatim into `ai_re/nix`.
- 2026-11-15: `lab-power` now needs a plug number. Plug 2 on/off tested,
  plug 1 unchanged (HA state only).
