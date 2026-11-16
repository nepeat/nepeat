# Reimaging a DMC-12 with NixOS

`reimage.py` does this end to end for one unit over its serial console. It is
for humans (asks before each write) and agents (`--yes --json`). The system
is the generic `nix/configs/dmc12` (hardware: `nix/modules/boards/dmc12`).

## Before the first unit
```sh
# from ai_re/: the root flake (git flake: fand is in; `path:./nix` refuses DMC-12 systems)
R="git+file://$(git rev-parse --show-toplevel)?dir=ai_re"
nix build "$R#dmc12-netboot"    -o devices/dmc-12-48r-as0/nixos-build/result-netboot
nix build "$R#dmc12-disk-image" -o devices/dmc-12-48r-as0/nixos-build/result-disk-standard
# units without our U-Boot in SPI: the chainload layout (u-boot-port.md "on-disk boot"):
nix build "$R#dmc12-disk-image-chainload" -o devices/dmc-12-48r-as0/nixos-build/result-disk-chainload
# new units (provision.py): our U-Boot for the chainload and for the SPI IPXE slot
nix build "$R#uboot-dmc12-chainload"  -o devices/dmc-12-48r-as0/nixos-build/result-uboot-dmc12-chainload
nix build "$R#uboot-dmc12-stage3app"  -o devices/dmc-12-48r-as0/nixos-build/result-uboot-dmc12-stage3app
```
The default `--image` is the standard build (needs `spi-upgrade.py install` on the unit first; the
script checks); pass `--image nixos-build/result-disk-chainload` otherwise.
- Console: one logged tio in tmux `serial-dmc12` (PROGRESS Status). The FTDI
  cable goes to the unit's front console port, 115200 8N1.
- Power: the unit's lab plug in `tools/lab_power.py` (`--plug N`), or reset it
  by hand and use `--no-power`.
- TFTP: atftpd serves `/srv/tftpboot`. The script stages into `dmc12/` (public
  build artefacts only; no keys, no dumps).

## Per unit
New units: `provision.py` runs all of this in order (units.md "New units"); by hand:
1. **Original state first.** Capture it read-only per `units.md` (`acquire.py`:
   sda, mtd0, mtd1) and add the serial to `units-private.txt`. The script
   refuses to write without both.
2. `./reimage.py --unit u02 --plug 3` (add `--yes` to run unattended).

What it writes, and only after confirmation:
- `/dev/sda`: the whole disk image, read back and sha256-checked on the box.
- U-Boot env, two variables, nothing else: `bootcmd=run nixos_boot; run dmc12_bootdisk`
  and `nixos_boot`, which depends on the image:
  - uboot-menu: `scsi init; ext4load scsi 0:1 0x07000000 /boot.scr && source 0x07000000`
  - chainload: the image's `factory-env.txt` (factory preamble, then `scsi read` of
    U-Boot slot A, else B, from raw sda1, and `bootm`).

  The previous env is saved first to
  `inventory/<unit>/uboot-printenv-before-reimage-private.txt`.

Everything before `write` is non-destructive: the netboot runs from RAM.
`--spi-next` (what provision.py uses): a standard image on a unit whose SPI still has only the
factory U-Boot. It writes the disk and skips setenv/boot. Until `spi-upgrade.py install`
succeeds, the box can't boot from disk (rehearse boots it from RAM); to undo, "Back to factory".
It records the result in `inventory/<unit>/reimage.jsonl`.

## Back to factory
- Disk: netboot, then `zstd -dc --long=27 dumps/<unit>/sda.img.zst | ssh root@<ip> 'dd of=/dev/sda bs=4M conv=fsync'`.
- Env (U-Boot): `setenv bootcmd 'run dmc12_bootdisk'; setenv nixos_boot; saveenv`.
  With the factory disk back, the old bootcmd alone would also work: `nixos_boot`
  fails (no /boot.scr, or no valid uImage at the slot LBAs) and the chain falls
  through to `dmc12_bootdisk`.
- Units with our U-Boot in SPI: `spi-upgrade.py --unit <u> --plug <n> rollback --write` first
  (stage3 boots the factory U-Boot again); `mtd0.bin.zst` is the full backup for anything else.

## Updating an installed unit
In place, from the workstation (`profile-rebuildable`; root's key is the one the box trusts):
`./deploy.sh <ip> [switch|boot]` (`boot` + reboot for kernel/DTB changes): it builds from the ai_re root
flake as you (so fand is in: `path:./nix` would fall back to the python fan script), copies with
root's key and activates — nixos-rebuild --target-host's steps. It copies only new paths; generations appear in
the extlinux menu; on the box `nixos-rebuild switch --rollback --no-reexec`. Reimage only to change the
disk layout. In chainload mode an update also writes a changed U-Boot to raw slot A (old A to B).
