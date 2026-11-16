# Edge-Core 7760-32X

Start with Status below; [HANDOFF.md](HANDOFF.md) has access, deploy and recovery
for the next session.

## Status (2026-11-15)

- **Runs:** upstream U-Boot 2026.10 in boot NOR (`uboot-as7760-nor`, sha256
  `2c0d7387…`, crc32 `31a7d1c9`) → `bootflow scan -lb` → extlinux on SD p1 →
  NixOS 26.11 ([config](../../nix/configs/as7760/configuration.nix)), 0 failed units.
  At boot: OpenBCM kmods, fand, node-exporter :9100 (lab VictoriaMetrics) and SONiC
  (`services.sonic`, syncd owns the ASIC: [sonic-nixos](../sonic-nixos/PROGRESS.md)).
- **Reach:** `sudo ssh root@10.60.2.253`. Serial: the cable is on DMC-12 port 48, not
  served yet (ask the DMC-12 session); until then no U-Boot work or risky reboots.
  Power: `lab-power 1 …` (`tools/lab_power.py`). Details: [HANDOFF.md](HANDOFF.md).
- **Change it:** `./deploy.sh` (Nix in [ai_re/nix](../../nix/README.md)),
  `./flash_uboot.py` (U-Boot, from the console), `./chainload_uboot.py` (RAM test first).
- **Recover:** NixOS: `nixos-rebuild --rollback` or an older extlinux entry (console).
  U-Boot: `flash_uboot.py --restore` (`dumps/flash-backup-20261114-pre-0013/`) from a
  prompt, else clip/JTAG at NOR 0x7F40000. Factory U-Boot (`dumps/mtd7.bin`) restore is
  unsupported since 2026-11-15. SD original `dumps/mmcblk0.img`; ONIE untouched.
- **Fans:** fand (board + ASIC zones, 100% on fault) at the 53.3% minimum, ~30 °C board,
  ~42 °C ASIC; ASIC reading from syncd, else `openbcm-asic-temp` ([thermal.md](thermal.md)).
- **Front panel:** only the lab loop (cage 1 4x25G breakout, leg 1 into a QSA in cage
  32, 25G); PSU 2 has no power cord.
- **Runbook** ([switch-porting-runbook.md](../../switch-porting-runbook.md)): gates 1-3, 4b, 5, 6
  hold; 4 short of RTC, watchdog, TPM, CPLD LEDs; 0 lost its console on 2026-11-15; 7 done
  but restore-from-Linux and a JTAG rehearsal; 8 past its gate (SONiC on the ASIC).
- **Held / todo:** the software over-temperature PLL shutdown (`openbcm-asic-temp
  --shutdown-at`, never fired on hardware) waits for the console; a worst-case thermal
  test needs line-rate traffic (the 2026-11-15 loopback storm was throttled).
- **Open:** `WARNING: adjusting available memory` at boot is stock mpc85xx (harmless);
  other T2080 boards need their own DDR/DT patches ([u-boot-port.md](u-boot-port.md)).

## 2026-11-15 (night) — new kernel booted, loopback load test (mild)

- Rebooted without a console (owner's go) into the kernel with bridge,
  VLAN, team and dummy modules: up in ~2 min, 0 failed units.
- Load test via [loadtest.sh](loadtest.sh): 10 min MAC-loopback storm, ASIC
  +2.4 °C, fans at the minimum, no failsafe; throttled by pause/queue drops,
  so not a worst case ([thermal.md](thermal.md)).

## 2026-11-15 (evening) — SAI temperature + LAG on hardware, QSPF control, fand grace

- Through syncd (sai-openbcm): ASIC temperature attributes (8 sensors, max
  39 / avg 37 °C) and a LAG with two members on SDK trunk 0, member
  egress-disable, teardown: SMOKE_OK ([transcript](../sonic-nixos/sai-lag-temp-hw-20261115.txt)).
  syncd publishes `/run/asic/temperature`, so fans stay at the minimum while
  SONiC owns the chip. LAG traffic and LACP not tested yet.
- fand: sensor `grace` (AS7760 asic: 15 s) bridges writer restarts without a
  failsafe blip; PI back-calculation (no AS7760 zone uses PI).
- QSFP: CPLD#1 reset/presence/interrupt via `as7760-qsfp --status/--reset`
  ([qsfp-control.md](qsfp-control.md)); no LPMODE pin, software low power
  untested on an active module.
- Load test (MAC-loopback broadcast storm via `bcm.user`) **not done**: the
  diag shell hung at `tx` with all 32 ports in loopback (no readings reached
  fand, which held 100% as designed; chip reset with `init all` each time).
  Next attempt through syncd + a hostif seed packet.
- Factory U-Boot restore is unsupported now (flash_uboot.py refuses it).
- Serial: the console cable went to the DMC-12 project around 10:15 UTC; the
  AS7760 console is DMC-12 port 48 (not live yet). Deploys over SSH only; no
  U-Boot or power-cycle experiments until the console is back.

## 2026-11-15 (later) — ASIC die temperature drives the fans

- SDK kmods load at boot now (module `switch/openbcm.nix`); `openbcm-asic-temp`
  reads the eight PVTMON monitors every 5 s, attach only, and feeds fand zone
  `asic` (required sensor: no reading = 100%). One-SDK-process lock
  `/run/openbcm-unit0.lock` (refused a second instance in a live test).
- Cold power cycle found the reader broken: S-bus ring map zero on a chip
  nothing initialised, every TOP read timed out (fand went to failsafe as
  designed). Fixed by programming the ring map + PVTMON trim as the SDK's chip
  reset does, only when zero; a second cold cycle reads 37–40 °C. Details:
  [thermal.md](thermal.md).
- fand: hardware minimum folded into zone targets, start-up warnings for
  settings below it, `max_duty` capping failsafe/exit is a config error.
  AS7760: raw_min 136, profile none, board curve to 100% at 45 °C with
  critical 50 (was a jump to failsafe at 45), idle point on 53% (54% rounded up
  to the 60% CPLD step). Deployed `qy2gw3ys…-nixos-system-as7760`; 0 failed units,
  fans raw 136. Grafana thermals dashboard has ASIC panels.
- Open: SAI `TEMP_LIST` and publishing the temperature from syncd (fans go to
  100% while syncd owns the chip); traffic-load thermal test.

## 2026-11-15 — monitoring, then fand as the fan controller

- node-exporter (`monitoring-node-exporter`, `node-exporter-lite`: Go
  node_exporter dies with SIGILL on e6500) + `as7760-metrics` textfile producer
  (`switch_*`: optics, fans, board temps). 20 collectors OK, ~1170 series.
  Scraped every 10 s by the workstation VictoriaMetrics; Grafana dashboards.
- Baseline at 80% (8 min), then fand at its floor as a `systemd-run` test unit
  (10 min): board max +1.3 °C then flat. Numbers in [thermal.md](thermal.md).
- `hardware.as7760Fan.controller = "fand"` (default from the root flake);
  `deploy.sh` now builds `git+file://<monorepo>?dir=ai_re#as7760` so the flake
  reaches `rust/` (tracked files only: `git add -N` new ones).
- Workstation nix store was purged first, so the build rebuilt the whole
  cross toolchain (local log `nixos-build/build-as7760-fand-20261115.txt`, git-ignored, EXIT 0).
  Deployed `ikibc7z7…-nixos-system-as7760`: fand started at 100%, ramped to
  pwm 136 in < 2.5 min, board max 30.5 °C; test unit stopped first so two
  controllers never ran; `as7760-thermal` inactive; 0 failed units.

## 2026-11-14 (night, late) — standard boot complete, reflash tool, shared Nix tree

- U-Boot rebuilt and reflashed (patch 0013 + config): real board name, no RDB
  CPLD access, no `cpld`/`mac` commands, no ID-EEPROM probe, `bootdev`/`bootmeth`
  commands, extlinux + script + PXE boot methods, load addresses built in, no
  SATA scan. Env reduced to `nos_bootcmd=bootflow scan -lb`. Reset, 2 cold
  cycles and a warm reboot OK; `ddrcmp` 0 diffs.
- [flash_uboot.py](flash_uboot.py): console reflash tool for humans (image,
  SVR, RCW, identity, flash-map and DIMM checks; verified console backup; RAM
  rollback copy; typed confirmation). Dry run and real flash done with it here.
- RAM testing works from the upstream loader again: the chainload build hung
  for three separate reasons (used L2 kept, parent's DDR TLB slot overlapping
  ours, CPC invalidated with dirty lines); all fixed in patch 0001. Two of my
  debug conclusions on the way were wrong (UART FIFO overflow dropped marker
  characters; a debug TLB entry overlapped the CCSR one) — see u-boot-port.md.
- Linux DT: removed `adt7461@4c` / `eeprom@50` (RDB leftovers, nothing answers
  there): the `lm90 … Failed to initialize` boot error is gone.
- Nix moved to [ai_re/nix](../../nix/README.md) (profiles / platforms / functions /
  boards). Checked neutral: same U-Boot hash as flashed, same kernel `.drvPath`,
  closure diff = intended changes only (stage-2 U-Boot dropped from the system:
  −250 MB; `fastfetch-tiny` added). Flakes enabled on the target.
- Not a bug: DHCP right after power-on fails until the uplink port forwards (~30 s).

## 2026-11-14 (evening, late) — upstream U-Boot 2026.10 flashed to boot NOR; boots NixOS directly

- **Boot NOR `uboot` (mtd7, 0xEFF40000, 768 KiB) now holds upstream U-Boot
  2026.10** (`#uboot-as7760-nor`, sha256 `1e644c49…`, crc32 `a959900c`). Written
  from the factory prompt (tftp new + factory copy to RAM, crc32 both, `cmp.b`
  flash vs backup, `protect off; erase; cp.b; cmp.b; crc32; protect on`).
  Transcript [nor-flash-20261114.txt](nor-flash-20261114.txt). Factory image:
  `dumps/mtd7.bin` (sha `91d56cb2…`, crc32 `38c38261`), also staged as
  `/srv/tftpboot/as7760-probe/mtd7-factory.bin`.
- First cold boot from NOR: banner, SPD DDR init 16 GiB ECC, PCIe3 x4 gen2,
  Flash 128 MiB, factory env loaded OK (same 0x2000 env format), FMan ucode.
  `ddrcmp`: **0 registers differ**, CPO 0x4d; `mtest` 64 MiB 1 pass 0 errors.
  Then extlinux → NixOS: 8 CPUs, 15 GiB, 2 ASIC functions, 0 failed units.
  Warm `systemctl reboot` + 2 unattended cold power cycles all booted.
- Found on first boot: `bootdev` is not a command in this build (CMD_BOOTDEV
  off), so the env's `if bootdev list` picked the factory branch. Changed
  `nos_bootcmd` to `if bootflow list; …` and `saveenv` from the new U-Boot
  (first env write by it; OK). Env backups: `dumps/mtd6-env-20261114-dual.bin`
  (before), `…-nor-uboot.bin` (after).
- Before the flash: the intermittent silent hang after factory `fatload; go` of
  stage 2 was **start.S flash-invalidating an L2 that held dirty lines of the
  image itself** (tftp flushes caches, fatload does not). Patch 0001 now keeps
  an already-enabled L2 in the chainload build; 3/3 unattended boots after.
- Cosmetic/known: banner says `Board: T2080RDB … CPLD ver: 0xff, boot from NAND`
  (no RDB CPLD here), `EEPROM: Read failed`, `WARNING: adjusting available
  memory from 0x30000000 to 0x80000000`, kernel `lm90 0-004c: Failed to
  initialize device`. Not investigated yet. `cpld reset` untested — don't use.
- Recovery if it ever fails to boot: JTAG/flash clip, write `dumps/mtd7.bin` at
  NOR offset 0x7f40000. From a working prompt: same procedure as the flash
  with `mtd7-factory.bin`. SD p1 still carries stage-2 `u-boot.bin` (only used
  by the factory U-Boot).

## 2026-11-14 (night, cont.) — upstream U-Boot is now the second boot stage by default

- Default chain: factory U-Boot → `nos_bootcmd` loads **upstream U-Boot 2026.10
  from SD p1 (`u-boot.bin`, installed by the NixOS hook)** → board identity from
  factory env → **extlinux `bootflow scan`** → NixOS. Power-on and reboot
  verified; 8 CPUs, 16 GiB, correct MAC, 0 failed units. Fallbacks documented in
  [u-boot-port.md](u-boot-port.md). Factory env changed once more (saveenv).
- DDR: new `ddrcmp` command shows the AS7760 DDR settings (patches 0010/0011)
  now match the factory controller setup except two benign bits.
- `spltest` cold-DDR-init experiment (no NOR writes) faults inside the SPL
  reset path; open. Boot NOR still never written.

## 2026-11-14 (late night, cont.) — upstream U-Boot boots NixOS; extlinux standard boot

- Chainloaded upstream U-Boot 2026.10 now **boots NixOS fully**: 8 CPUs, 16 GiB,
  ASIC PCIe ×2, correct MAC, 0 failed units — via plain bootm, via the
  **factory env's ONIE bootcmd** (`env import` from NOR, read-only), and via
  **standard boot (`bootflow scan -lb` → extlinux)**. Patches 0006–0008 + config;
  details in [u-boot-port.md](u-boot-port.md). NOR still untouched.
- NixOS now also installs **extlinux** (NixOS's own generic-extlinux builder,
  4 generations, FDT entry) to SD p1 alongside the factory boot.scr; kernel
  package ships a legacy uImage (`kernelFile = "uImage"`); p1 is the bootable
  partition. Old pre-extlinux profile generations 1–3 removed (builder needs a
  DTB per generation). Factory autoboot verified unchanged afterwards.
- No UEFI possible on PowerPC (U-Boot EFI_LOADER / Linux EFI stub absent);
  extlinux is the standard path.

## 2026-11-14 (late night) — installed NixOS is the default boot; rebuilds in place; upstream U-Boot runs via chainload

- **Normal NixOS install on the SD** ([sd-install.nix](../../nix/modules/boards/as7760/sd-install.nix), nixpkgs
  `sd-image` module): p1 FAT `NIXOS_BOOT` 256 MiB (uImage/uInitrd/DTB/`boot.scr`,
  `prev/`), p2 ext4 `NIXOS_SD` grown to 7.0 G (5.3 G free). nix daemon on the
  target. Image 2.45 GB written from the RAM-root system, readback SHA256
  `242c2946…` match.
- **Default boot**: factory env `nos_bootcmd='mmc rescan; fatload mmc 0:1 0x07000000
  boot.scr && source 0x07000000'` + `saveenv` (only change; ONIE hook, ONIE stays
  the fallback; env backup `dumps/mtd6.bin`). Unattended power-on and
  `systemctl reboot` both autoboot NixOS. 0 failed units.
- **Rebuild in place**: [deploy.sh](deploy.sh) = `nixos-rebuild switch --target-host`
  (cross-built here). Bootloader hook copies the generation's `as7760-boot/` files
  to p1, moves the old generation to `prev/` (own `boot.scr`). Verified with a
  real kernel change (gen 3). Rollback at the factory prompt:
  `fatload mmc 0:1 0x07000000 prev/boot.scr; source 0x07000000`.
- Fixes found on the way: hook needs `mkdir -p /boot/firmware` (noauto mount);
  first-boot `expand-root-partition` failed: `resize2fs` EBUSY because the
  allnoconfig kernel lacked **`BLK_DEV_WRITE_MOUNTED`** (now on; resized online).
  Dropped the sd-card name tag (bootargs 232 chars; U-Boot console limit 256).
  `ubootTools` cross-build fails (python-cryptography Rust vs ppc64 ELFv2), so
  no fw_printenv on target yet.
- SD writes: post-resize burst (~60 MiB lazy init) then **~12 KiB/min** steady
  (ext4 commits of /var state). Journal volatile, 1 GiB.
- **Upstream U-Boot 2026.10 runs on the T2080** (chainloaded from the factory
  U-Boot, RAM only): reaches `=>`, correct clocks/RCW, sizes 16 GiB from both
  UDIMM SPDs, DM, MMC, I2C, FMan ucode from NOR, memory fill/compare OK.
  [Details + patches](u-boot-port.md), transcripts `chainload-*.txt`.
  Static + QEMU-e6500 tests: `#uboot-tests`. Nothing written to NOR.

Older entries (first boot, acquisition, RAM boots, netboot, RX misdiagnosis,
RAM-root SD image): [HISTORY.md](HISTORY.md).

## NOR map (reference)

| MTD | Offset | Bytes | Name |
| --- | --- | --- | --- |
| 0 | 0x0000000 | 131072 | RCW |
| 1 | 0x0020000 | 131072 | hw-info |
| 2 | 0x0040000 | 33554432 | onie |
| 3 | 0x2040000 | 33554432 | diag |
| 4 | 0x4040000 | 65798144 | open |
| 5 | 0x7f00000 | 131072 | Fman-FW |
| 6 | 0x7f20000 | 131072 | uboot-env |
| 7 | 0x7f40000 | 786432 | uboot |

`/etc/fw_env.config`: `/dev/mtd6 0x0 0x2000 0x20000`.

Access notes and the 2026-11-14 acquisition record (hashes, contents): [HISTORY.md](HISTORY.md#sections-moved-from-progressmd-on-2026-11-15-written-2026-11-14).
