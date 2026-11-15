# Reimaging a DMC-12 with NixOS

`reimage.py` does this end to end for one unit over its serial console. It is
for humans (asks before each write) and agents (`--yes --json`). The system
is the generic `nix/configs/dmc12` (hardware: `nix/modules/boards/dmc12`).

## Before the first unit
```sh
nix build path:./nix#dmc12-netboot    -o devices/dmc-12-48r-as0/nixos-build/result-netboot
nix build path:./nix#dmc12-disk-image -o devices/dmc-12-48r-as0/nixos-build/result-disk
```
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
- U-Boot env, two variables, nothing else:
  `nixos_boot=scsi init; ext4load scsi 0:1 0x07000000 /boot.scr && source 0x07000000`
  and `bootcmd=run nixos_boot; run dmc12_bootdisk`. The previous env is saved
  first to `inventory/<unit>/uboot-printenv-before-reimage-private.txt`.

Everything before `write` is non-destructive: the netboot runs from RAM.
It records the result in `inventory/<unit>/reimage.jsonl`.

## Back to factory
- Disk: netboot, then `zstd -dc --long=27 dumps/<unit>/sda.img.zst | ssh root@<ip> 'dd of=/dev/sda bs=4M conv=fsync'`.
- Env (U-Boot): `setenv bootcmd 'run dmc12_bootdisk'; setenv nixos_boot; saveenv`.
  With the factory disk back, the old bootcmd alone would also work: `nixos_boot`
  fails (no /boot.scr) and the chain falls through to `dmc12_bootdisk`.
- SPI was never written except the env sector. `mtd0.bin.zst` is the full
  backup if anything else ever is.

## Updating an installed unit
Rebuild the disk image and reimage. The appliance profile disables on-device
switching (`system.switch.enable = false`), so an in-place update needs a deploy
script, not written yet. It would `nix copy` the closure, then run the
bootmenu builder (`system.build.installBootLoader <toplevel>`) and reboot.
