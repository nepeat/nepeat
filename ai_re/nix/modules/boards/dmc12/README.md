# board-dmc12: DMC-12-48R-AS0 (Annapurna Alpine V2, 48-port console server)

Started as a verbatim import of [codeberg dma/dmc12-nixos](https://codeberg.org/dma/dmc12-nixos)
at `5d02d1b3` (2026-11-15); `git log -p -- .` shows every change since. Exported as
`nixosModules.board-dmc12`; the complete system is `nix/configs/dmc12` (board +
`profile-appliance`; the installed hosts add `profile-rebuildable`, the netboot one does not), outputs `dmc12-disk-image`, `dmc12-netboot`.
Device notes and dumps: `devices/dmc-12-48r-as0/` (not read from here, by rule).

## What it does
| file | what |
|---|---|
| `default.nix` | kernel options for the board, DTS (`packages/linux-alpine/boards/dmc12`), imports below |
| `bootloader.nix` | `hardware.dmc12.bootLoader`: **`standard`** (default: our U-Boot in SPI, standard boot finds nixpkgs' generic-extlinux-compatible on sda1), `chainload` (factory U-Boot env loads our U-Boot from a raw sda1), `uboot-menu` (factory U-Boot only, first iteration) |
| `boot-standard.nix`, `boot-chainload.nix`, `uboot-menu/` | the three boot paths |
| `disk.nix` | SSD image per boot path (MBR, `DMC12BOOT` + `DMC12ROOT`), first-boot store registration |
| `uboot-env/` | our U-Boot's env from Linux: DT overlay (only `u-boot-env` 0xf90000+0x20000 writable), `fw_printenv`, `dmc12-uboot-env print\|set` (logged, counted, >20 writes/day need `--force`) |
| `cpld/` | CPLD board reset (kernel reboot notifier), interrupt monitor (alarms logged once per change, uplink re-sync) |
| `fan-control/`, `../../thermal/dmc12-fan.nix` | fan policy (uC 60 °C, board 45 °C) |
| `dmc12-tools/` | `show`, `setled`, `setphyspeed`, `onieeeprom`, boot oneshots |
| `monitoring.nix` | node_exporter + `dmc12_*` producer (quiet: `LogLevelMax=notice`) |

Our U-Boot itself (SPI IPXE slots) is not installed by NixOS: `devices/dmc-12-48r-as0/spi-upgrade.py`.

## Known gaps
- DT: no `gpio-leds`, thermal zones, SGPO or the second TPM (`infineon,slb9670` on SPI CS2) yet;
  each is a decision (describe it or leave it out on purpose).
- Ported factory scripts (fan floor at step 9, `setphyspeed -q`) match factory OS >= 1.0.526; units
  on 1.0.457 lack both, the module enforces them itself.
- Hardware assumptions (I2C bus numbers pinned by DT aliases to the factory numbering, EEPROM
  `7-0051`, sensors 0x48/0x49/0x4a, the USB hub topology behind P1-P48) are checked on u01 only.
