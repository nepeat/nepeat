# Reimaging a DMC-12 with NixOS

`reimage.py` does this end to end for one unit over its serial console. It is
for humans (asks before each write) and agents (`--yes --json`). The system
is the generic `nix/configs/dmc12` (hardware: `nix/modules/boards/dmc12`).

## Before the first unit
```sh
nix build path:./nix#dmc12-netboot    -o devices/dmc-12-48r-as0/nixos-build/result-netboot
nix build path:./nix#dmc12-disk-image -o devices/dmc-12-48r-as0/nixos-build/result-disk-standard
# units without our U-Boot in SPI: the chainload layout (u-boot-port.md "on-disk boot"):
nix build path:./nix#dmc12-disk-image-chainload -o devices/dmc-12-48r-as0/nixos-build/result-disk-chainload
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
It records the result in `inventory/<unit>/reimage.jsonl`.

## Back to factory
- Disk: netboot, then `zstd -dc --long=27 dumps/<unit>/sda.img.zst | ssh root@<ip> 'dd of=/dev/sda bs=4M conv=fsync'`.
- Env (U-Boot): `setenv bootcmd 'run dmc12_bootdisk'; setenv nixos_boot; saveenv`.
  With the factory disk back, the old bootcmd alone would also work: `nixos_boot`
  fails (no /boot.scr, or no valid uImage at the slot LBAs) and the chain falls
  through to `dmc12_bootdisk`.
- SPI was never written except the env sector. `mtd0.bin.zst` is the full
  backup if anything else ever is.

## Updating an installed unit
In place, from the workstation (`profile-rebuildable`; root's key is the one the box trusts):
`nix build path:./nix#nixosConfigurations.dmc12.config.system.build.toplevel` first (as yourself: root
can't take the store locks here), then `sudo nixos-rebuild switch --flake path:./nix#dmc12 --target-host
root@<ip>` (`boot` + reboot for kernel/DTB changes). It copies only new paths; generations appear in
the extlinux menu; on the box `nixos-rebuild switch --rollback --no-reexec`. Reimage only to change the
disk layout. In chainload mode an update also writes a changed U-Boot to raw slot A (old A to B).
