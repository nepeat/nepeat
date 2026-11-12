# tn48m -- Delta TN48M (S/N TN481N1TW21290148) running NixOS on nixpkgs master
# with the stock linuxPackages_latest. Hardware profile: module/tn48m.
#
# Bring-up stage: boot + console + management Ethernet (end2) first, then the
# Prestera front panel. No front-panel port is configured or bridged here, so
# the ASIC does not forward between them.
#
# Install is a raw MBR image, not an installer:
#   nix build .#nixosConfigurations.tn48m.config.system.build.sdImage
# then write it to the SATA SSD (/dev/sda) from ONIE. ONIE's own DTB lacks
# the ATF reserved-memory node and panics under bulk I/O, so boot ONIE with
# the RAM-patched DTB first (ai_re/devices/delta-tn48m/uboot.md).
# p1 = ESP (vfat "ESP", MBR type EF, 512 MiB) mounted at /boot, holding
#      /extlinux/extlinux.conf + /nixos/* (kernels, initrds, DTBs);
# p2 = ext4 root ("NIXOS_SD"), grown to the whole disk on first boot.
#
# Boot: the factory U-Boot (Amazon-signed, secure-boot fused, can't be
# replaced) runs extlinux via `sysboot` from nos_bootcmd (SPI env, set once):
#   fw_setenv nos_bootcmd 'setenv ramdisk_addr_r 0x10000000; scsi reset; sysboot scsi 0:1 any 0x0f000000 /extlinux/extlinux.conf'
# If it fails, bootcmd falls through to ONIE; `fw_setenv nos_bootcmd true`
# restores the as-received value. Its 2018.03 `bootefi` is too broken for
# systemd-boot (ai_re PROGRESS 2026-11-11 (c)); real EFI is a later step via a
# chainloaded mainline U-Boot.
# U-Boot autoboot password: @wa23.
{
  lib,
  pkgs,
  modulesPath,
  config,
  ...
}:

{
  system.stateVersion = "26.11";

  imports = [
    (modulesPath + "/installer/sd-card/sd-image.nix")
    ../../module/tn48m
  ];

  # Rolling: newest mainline from nixpkgs master (cached on cache.nixos.org).
  boot.kernelPackages = pkgs.linuxPackages_latest;

  hardware.tn48m = {
    boot = "extlinux";
    thermal.enable = true;
    # Leave the watchdog off until the boot path is proven; a 10 s fixed
    # timeout plus a half-working userspace would be a reset loop.
    watchdog.enable = false;
  };

  # Disk image from the sd-image builder. Its FAT "firmware" partition is the
  # ESP (/boot), retagged as MBR type EF so a later systemd-boot/bootctl
  # accepts it. sd-image also registers the store paths and grows the root on
  # first boot.
  image.baseName = "nixos-tn48m";
  sdImage = {
    compressImage = false;
    firmwareSize = 512;
    firmwarePartitionName = "ESP";
    populateFirmwareCommands = ''
      ${config.boot.loader.generic-extlinux-compatible.populateCmd} \
        -c ${config.system.build.toplevel} -d ./firmware
    '';
    # No /boot inside the root fs: /boot is the ESP.
    populateRootCommands = "";
    postBuildCommands = ''
      sfdisk --part-type $img 1 EF
    '';
  };

  fileSystems."/boot/firmware".enable = false;
  fileSystems."/boot" = {
    device = "/dev/disk/by-label/ESP";
    fsType = "vfat";
    options = [
      "fmask=0077"
      "dmask=0077"
    ];
  };

  # Each generation with a new kernel puts ~100 MB (Image + initrd) on the
  # 512 MiB ESP.
  boot.loader.generic-extlinux-compatible.configurationLimit = 3;

  # Management port only. Prestera front-panel netdevs are left unmanaged
  # (networkd ignores links nothing matches), i.e. admin-down.
  networking.useDHCP = false;
  networking.useNetworkd = true;
  systemd.network = {
    enable = true;
    wait-online.enable = false;
    networks."10-end2" = {
      matchConfig.Name = "end2";
      networkConfig = {
        DHCP = "yes";
        IPv6AcceptRA = true;
      };
      dhcpV4Config.ClientIdentifier = "mac";
    };
  };

  # Serial console is physical access (which already means U-Boot access);
  # every network login is SSH-key-only.
  services.getty.autologinUser = "root";

  # Same aarch64 lldpd build fix as cs8210 (bundled linux/types.h lacks
  # __u128); drop together with that one.
  nixpkgs.overlays = [
    (final: prev: {
      lldpd = prev.lldpd.overrideAttrs (old: {
        patches = (old.patches or [ ]) ++ [ ../cs8210/lldpd-bundled-types-u128.patch ];
      });
    })
  ];

  programs.nix-index.enable = lib.mkForce false;
  programs.nix-index-database.comma.enable = lib.mkForce false;
  documentation.nixos.enable = false;

  # Same root keys as cs8210: https://github.com/nepeat.keys (fetched
  # 2026-11-09) plus root on sea420-desktop, the build/deploy host.
  users.users.root.openssh.authorizedKeys.keys = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMVk9i7FG7dc9r4ixwAJT7uPLH3UuqbwIgeZ7Ytmnpvv"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBdCw3FOLZYLg9E9VSbJYhFFFOFYOS+F0Fzi7PZPW+tv"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAII1xDf7IxdQU/hIvDYL6VCzfV14ZbfyLxJnIYFsBHqxI"
    "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBI6/FNMHDsQ/b6I2vjOeTyyQh02kdSDKRR2FQHGe72DLwypxHO7QTYnhVs9q5GRsFRC0Dj8zwBg+ve/9Ny3I2b0="
    "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQCkEDEKqOyMe6QHZ9stWPbm+lE+NKoEer6BRW8LmEAZ7ekxjY+OiBmh0sBLMBbeOMannm7fvQ27bShZCnglDaW9k5Rf2d7vdmRBbvGkNjvg6nbFJT6M64yi8jxPt2FixDhrkcDlFUbKCZMGEJ6z5vSHm5pqDnRXdse3KPQ+Ul8h5pSfM31Nubto4onhhu0xfFuLorxUFD1/opvLs9y/geqQQ8T+zu5Ms05MfVvGD1S+Uo+EieOnhore7KdcfhG4Fi0AkwpfVVB91DS6SVSQJSbVIKLo366PVEl9JdAn59ZvaEOxq70yW36YOIRAFAm1ANLYj3jhbxf68ds7+RjgOqXH"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA60KDLPhZaDPbstXgHVK4RnZq3dSzfbVzfVBheWOMb+ root@sea420-desktop"
  ];
}
